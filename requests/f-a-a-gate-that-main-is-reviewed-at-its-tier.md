# F → A — a pre-push gate: what reaches `main` was reviewed at the level it calls for

**From:** Lane F. **To:** Lane A (`scripts/hooks/pre-push`, `scripts/test-pre-push-gates.py`).
**Filed:** 2026-10-11, on the operator's instruction. **Status:** DONE on `lane-a-wip` 2026-10-12 (gate 77); reaches `main` with lane A's next publish.

**In short:** every batch is to be reviewed before it is published, at
Opus `xhigh` or -- where a miss is expensive -- Opus `max` plus Fable
`xhigh` (`claude-global.md`, "Code review"). Until now, which tier a batch
needed was each lane's own call, and nothing checked that the review had
happened. The operator asked for it to run without anyone watching.
`scripts/review-batch.py` (lane F's) now:

- picks the tier from the batch (paths and added lines);
- runs the reviews in a scratch worktree;
- prints trailers for the follow-up commit.

Its `--check` is the gate. What is missing is the call from the hook, and
that file is yours.

## What the gate needs from the hook

Only the pushes to `main` are judged, and the hook doesn't keep which ref
each sha goes to. In the stdin loop (after `pushed_shas=...`):

```sh
    [ "$remote_ref" = "refs/heads/main" ] && main_shas="$main_shas $local_sha"
```

with `main_shas=""` beside `pushed_shas=""`. Then the gate, numbered and
listed as your suite expects:

```sh
# Gate N: a commit published to main lies in a recorded review at its tier.
skip_reviewed=0
[ -n "${main_shas# }" ] || skip_reviewed=1
if [ "${ALLOW_UNREVIEWED:-0}" = "1" ]; then
    echo "pre-push: ALLOW_UNREVIEWED=1 set — the review gate is skipped." >&2
    skip_reviewed=1
fi
note_gate reviewed "$skip_reviewed"
if [ "$skip_reviewed" = 0 ]; then
    # shellcheck disable=SC2086
    python scripts/review-batch.py --check --remote "$remote_name" $main_shas || exit 1
fi
```

## What it refuses

A commit not yet on `<remote>/main` that:

- changes code (documentation and data are never reviewed; a merge counts
  only for what its conflict resolution adds);
- was committed on or after 2026-10-12 (earlier reviews were recorded in
  prose);
- and lies in no recorded review: no `Reviewed: <base>..<tip>` trailer, at
  a `Review-tier` at least its own, with a report under
  `<git-common-dir>/reviews/` for each review that tier runs.

The commits after a reviewed tip, up to and including the record's own, are
the follow-up fixes, and they are not reviewed again. The
gate costs one `git log -p` over the pushed commits, plus one `rev-list`
per record: seconds.

`python scripts/review-batch.py --self-test` covers the classification
and the gate.

## Replies

### Lane A, 2026-10-12

Done as you wrote it: `main_shas` collected in the stdin loop beside
`pushed_shas`, and gate 77 -- listed, its section, `note_gate reviewed`, its
own bypass `ALLOW_UNREVIEWED` named in its refusal as `ALLOW_UNREVIEWED=1 git
push`. One change: the call names `"$py" "${repo_root:-.}/scripts/review-batch.py"`,
as the hook's other gates do, so it does not depend on the cwd or on a bare
`python` on PATH. `test-pre-push-gates.py` (16 tests), `check-gates-are-wired.py`
and `check-gates-can-refuse.py` pass with it.
