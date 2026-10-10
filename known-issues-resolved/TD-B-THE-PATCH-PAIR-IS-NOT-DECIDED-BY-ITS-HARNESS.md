## TD-B-THE-PATCH-PAIR-IS-NOT-DECIDED-BY-ITS-HARNESS (lane B, 2026-09-11) — RESOLVED

**Status:** RESOLVED 2026-10-09

**Decided, and then made moot.** The standalone `userspace/patch` was retired
on 2026-09-12, which settled the pair in coreutils' favour. On 2026-10-09 the
coreutils half stopped being a reimplementation: it is now GNU patch 2.7.6 as
Ubuntu ships it, ported whole (design-decisions §1068). `patch-diff.sh` holds it
to that binary across ed scripts, git diffs, merges, every backup style and
every debugging flag, comparing output and the tree left behind, and it agrees
on every case but `--version`.

**A correction to the table below.** Its "a plain rebuild with no Debian source
patches" was wrong. Ubuntu's `2.7.6-7build3` is Debian's `2.7.6-7`, rebuilt, and
Debian carries fifteen patches on 2.7.6. Two of them fix security holes
(CVE-2018-1000156, CVE-2019-13636), one enables `-m`, and five fix crashes. The
port carries all fifteen, because that binary is what the harness measures
against. The entry is otherwise kept as written.

`scripts/patch-diff.sh` was written to settle the second of the two pairs where
`coreutils` is the thinner half. **It did not settle it**, and that is recorded
rather than rounded into a verdict.

65 cases against GNU patch 2.7.6 (Ubuntu `2.7.6-7build3`, a plain rebuild with
no Debian source patches — the least-diverged reference any harness here uses):

| half | passed | wrong tree on disk | wrong exit status | wording only |
|---|---|---|---|---|
| `coreutils` | 3 | **29** | 6 | 27 |
| standalone | 3 | **26** | 2 | 34 |

Three of 65 each. The standalone is *marginally* ahead where it counts — three
fewer wrong trees and four fewer wrong exit statuses — but this is nothing like
`diff`'s 43 against 21, and neither half is usable. **Do not act on a three-case
margin.** The honest reading is that `patch` needs work whichever half is kept,
and that a decision wants either a repaired implementation to compare or a
narrower harness aimed at the families above.

*The tree comparison is why those numbers mean anything.* `patch`'s output is a
**side effect**, so unlike every other harness here this one snapshots the whole
working tree after each case — every file, its mode, a hash of its bytes, and
the `.orig` and `.rej` files left behind. A `patch` that printed the right words
while writing the wrong bytes passes a stdout comparison, and 29 of coreutils'
62 differences are exactly that: right words, wrong files.

**Three defects in the harness itself, found by its own first run**, kept here
because each would recur in any future harness for a program that mutates state:

1. **It reported 0 passed of 65.** The generated patches were labelled
   `a/base.txt` while the tree held the file at `a/base.txt`, so `-p1` stripped
   `a/` and looked for `base.txt` at the root. Nothing was ever found. A
   uniform zero is a harness result, not a subject result, and should be read
   that way before any conclusion is drawn.
2. **GNU patch prompts on stdin when it cannot find the file**, and stdin was
   the patch — so it consumed the rest of the patch as answers to
   `File to patch:`. Fixed by driving with `-i FILE` and `</dev/null`, which
   needs no option either side might lack. Any harness for an interactive-capable
   program has to take stdin away from the prompt.
3. An apostrophe inside a single-quoted `xfail` reason terminated the string
   (shellcheck SC1011), which showed up as one "differ on purpose" instead of
   two. The same bug as in `diff-diff.sh`, written an hour earlier.
