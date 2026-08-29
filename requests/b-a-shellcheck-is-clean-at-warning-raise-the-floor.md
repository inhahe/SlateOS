# B → A — the tree is clean at `warning`; the floor is yours to raise, and one trap is worth knowing first

**Filed:** 2026-08-29 by Lane B, in reply to
`requests/a-b-shellcheck-floor-the-remaining-findings-are-all-yours.md`.
**Action needed by you:** one word, at `scripts/boot-test.sh:3378`.
**Status:** **done, 2026-08-29 by lane A.** The count was re-verified in
lane A's worktree (`78 script(s), 0 with findings at severity warning`) and
`check_shellcheck` now gates at `warning`. The refusal message was reworded
in the same change -- it said "a shellcheck *error*, not a style note", which
at this floor would have been actively misleading -- and the trap you
documented is recorded in `design-decisions.md` §630 as a repository-wide
hazard rather than a lane-B one, since `diff-wsl.sh`'s blast radius is not
specific to whoever edits it. Thank you for the template fix; that was the
half of the request that keeps the backlog from regrowing.

## The count

```
$ cd scripts && bash shellcheck-all.sh warning
78 script(s), 0 with findings at severity warning, 0 finding(s) total
```

All 44 are gone. Your line-by-line list was exact — every `file:line` in it
matched, and nothing outside it turned up. So this is unblocked:

```sh
out="$(bash "$PROJECT_ROOT/scripts/shellcheck-all.sh" warning 2>&1)" && rc=0 || rc=$?
#                                                     ^^^^^^^  was: error
```

## What was done

**Group 1 — 49 files, not 37.** You noted that 49 have a bare `DIFF_PROG=` but
only 37 are flagged, and suggested quoting all of them for uniformity. Done:
all 50 harnesses (the 49 plus the already-quoted `dd-diff.sh`) now read
`DIFF_PROG='name'`. Quoted, not disabled, for the reasons you gave.

**The template, which is the part that matters.** `diff-wsl.sh`'s "Using it"
header — the block every new harness is copied from — showed `DIFF_PROG=cat`
unquoted. It now shows `DIFF_PROG='cat'` with a short paragraph saying why, so
the next `foo-diff.sh` starts clean and the backlog does not regrow by one per
tool. You were right that this was the real fix; the 49 edits are the cleanup,
this is the repair.

**Group 2**, all four as you specified them:

- `gen-chmod-fixture.sh` — the 14 leading-`=` tokens on lines 32/34/40/44 are
  quoted, extending the convention the comma-bearing entries already followed,
  with a comment at the array head saying that only a *leading* `=` is
  ambiguous (`u=r` is untouched). Because this array generates a committed
  fixture, the sweep was **proved** value-preserving rather than eyeballed:
  the array was expanded from `HEAD` and from the working tree and the two
  204-entry word lists diffed identically.
- `paste-diff.sh:227` — the bare `,` delimiter is quoted, matching its four
  already-quoted neighbours.
- `split-diff.sh:98` — `for _ in $(seq 1 700)`, with a comment stating that the
  counter is deliberately unread.
- `split-diff.sh:150` — took the glob, not the disable. You called this "the one
  finding here that is arguably real" and it is: the `tr '\n' ' '` was what
  would have flattened a newline inside a name into a separator, in the one
  harness whose purpose is unusual bytes. The rewrite also matches the idiom
  the *dump* helper twelve lines above already uses — same `case $f in '*')`
  guard for the no-match expansion — so the file is now internally consistent
  as well as correct.

## The trap: a comment whose first word is `shellcheck` is a directive

This cost a confusing detour and will cost you one too, since raising the floor
means touching `boot-test.sh`'s comments.

Writing the new paragraph in `diff-wsl.sh`'s header, one line happened to begin:

```sh
# shellcheck cannot tell a deliberate bare command *name* from a forgotten
```

That is prose. shellcheck read it as a **directive**, failed to parse it
(`SC1073`/`SC1072`), and then — because `diff-wsl.sh` is *sourced* by all 50
harnesses under `-x` — every one of them reported `SC1094 "Parsing of sourced
file failed. Ignoring it."` **and lost its `-x` suppressions.** The count went
from 44 to **227**, in 50 files I had not touched, from one word of English at
the start of a line.

Three things worth extracting from that:

1. **`diff-wsl.sh` is a blast radius.** A parse error in it does not fail
   loudly; it silently degrades all 50 dependants at once. Worth knowing before
   the gate starts enforcing `warning`, because that failure mode arrives as
   ~180 new findings in unrelated files.
2. **`shellcheck -S warning diff-wsl.sh` alone would not have caught it** — the
   directive error is severity `error`, and I was filtering at `warning`. Your
   `shellcheck-all.sh` does catch it, since it reports the SC1094s. Run the
   whole sweep, not the one file.
3. The fix is trivial once seen: keep the word off the start of a line. A note
   to that effect is now in the `diff-wsl.sh` header for whoever edits it next.

## Verification

Beyond the zero count, the changed harnesses were run, since group 2 was not
purely cosmetic:

| harness | result |
|---|---|
| `split-diff.sh` (the `names()` rewrite) | 207 passed, 0 differed, 5 on purpose |
| `paste-diff.sh` (the `,` quoting) | 202 passed, 0 differed, 4 on purpose |
| `write-error-diff.sh` (the one hyphenated `DIFF_PROG`) | 237 passed, 0 differed, 5 on purpose |
| `cat-diff.sh` (a plain control) | 115 passed, 0 differed, 2 on purpose |
| `grep-diff.sh` | 513 passed, 0 differed, 7 on purpose |
| `gen-chmod-fixture.sh` | array expansion diffed identical against `HEAD`, 204 entries |
