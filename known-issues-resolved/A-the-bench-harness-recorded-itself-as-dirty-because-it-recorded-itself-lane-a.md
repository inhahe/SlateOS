## The bench harness recorded itself as dirty, because it recorded itself (lane A)

**Status:** FIXED 2026-08-19 (the mechanism). The ≤21 historical rows it
mislabelled are not recoverable; see the last section.

`boot-test.sh` stamps every row it writes to `bench/history.jsonl` and
`bench/boot-history.jsonl` with a `dirty` flag, from

```sh
git -C "$PROJECT_ROOT" diff --quiet HEAD 2>/dev/null || BT_DIRTY=1
```

Those two files are tracked, and they are the two files the harness appends to
on its way out. So the check included its own output: the first `--bench` run
at a commit is recorded clean, and it leaves the tree modified, so every run
after it says `"dirty": true` with nothing under test having changed.

The mislabel is **sticky, not per-commit**. The modification persists until
someone happens to commit `bench/history.jsonl`, which may be several commits
later — so a run's flag depends on whether an unrelated commit swept up the
record file, not on the tree it measured. In the current history the pure form
is still visible: `40515da89` has one clean row followed by five dirty ones and
`5e9a30a22` one clean then one dirty, with no source commit in between.

### Why it is not cosmetic

`dirty` means *the commit hash does not identify the source that was built*,
and consumers delete evidence on the strength of it:

| Consumer in `bench-history.py` | What a spurious `dirty` does |
|---|---|
| `layout_arms()` | drops the record outright — a layout band is only meaningful across arms of identical source |
| `same_image()` / `run_identity()` | refuses the `commit` fallback, so pre-`kernel_sha` rows stop matching each other |

The first is what made this urgent. A six-arm layout sweep runs `boot-test.sh`
six times at one commit; under the old check arm 1 was clean and arms 2–6 were
dirty, so `layout_arms()` would have kept **one** arm — two short of
`MIN_PADS_FOR_LAYOUT_BAND` — and `layout_bands()` would have returned `{}`.
Every consumer treats `{}` as "nobody measured this", which is correct and
which is exactly the problem: three hours of QEMU, no error, no band, and a
`--fail-on-regression` gate that goes on believing no layout sensitivity has
ever been measured. The project's recurring failure shape — a check that cannot
fire, presenting as a check that found nothing.

### The fix

The pathspec now excludes the harness's own records:

```sh
git -C "$PROJECT_ROOT" diff --quiet HEAD -- . \
    ':(exclude)bench/history.jsonl' \
    ':(exclude)bench/boot-history.jsonl' 2>/dev/null || BT_DIRTY=1
```

Narrowing `dirty` is the dangerous direction in general — a flag that fires
less often admits more records into comparisons — so the justification has to
be exact rather than merely plausible: these two files are never compiled and
never read by the kernel, so no edit to either can change what was built, and
what was built is the only thing `dirty` claims anything about.

`scripts/test-boot-test.py` covers it, and covers it by **extracting the check
from `boot-test.sh` rather than restating it**, so a rewrite of the line is
either exercised or fails loudly at extraction. It asserts both directions
(records excluded, kernel source still dirty, staged edits still dirty, seen
from an unrelated cwd) and that the excluded paths are still the ones
`bench-history.py` and `boot-history.py` actually write — the drift that would
otherwise reintroduce this silently under a new filename.

Two environment hazards were hit writing that test and are documented in it,
because both are the kind that make a harness bug look like a code bug: a
newline inside one argv entry does not survive to MSYS/WSL bash (the fragment
`BT_DIRTY=0\ngit …` arrives as `BT_DIRTY=0 git …`, a *temporary environment
assignment*, so `$BT_DIRTY` is never set), and the `bash` on PATH here is WSL,
which cannot open `C:\…` at all and whose git disagrees with Windows git about
line-ending normalisation.

### Not fixed: the rows already written

Of 90 rows in `bench/history.jsonl`, 21 carry `"dirty": true`. Six are provably
spurious (clean-then-dirty at one commit). The other fifteen are the first row
at their commit and so cannot be told apart from a genuinely uncommitted tree —
the stickiness means "first row at this commit" is not evidence of anything.
They are not recoverable: the only thing that could reclassify them is a diff
of a tree that no longer exists. They stay as they are, permanently excluded
from layout bands and from the `same_image` commit fallback, for the same
reason the 153 `sanitizer: null` rows above stay null — guessing would
manufacture exactly the certainty the flag exists to withhold.
