#!/bin/sh
# Commit the current todo2.txt to local-only history on the orphan branch
# `private/todo2`. Never pushed — the pre-push hook enforces that.
#
# Why an orphan branch, and why plumbing
# --------------------------------------
# The operator wants todo2.txt versioned in the local repo but kept off GitHub
# (2026-08-14). Committing it on `main` or a lane branch cannot work: those
# branches get pushed constantly by three agents, and every push would then
# either publish the file or be blocked by the hook — i.e. it would break
# pushing for everyone. So its history lives on a branch that shares no commits
# with the project at all.
#
# The branch is built with plumbing (hash-object / mktree / commit-tree /
# update-ref) rather than a checkout, because `git checkout private/todo2` in
# any worktree would rip the whole project out from under whichever agent is
# working there. Nothing here moves a HEAD, touches the index, or writes to the
# working tree.
#
# Usage, from any worktree:
#     ./scripts/snapshot-todo2.sh
#
# Read the history back without checking anything out:
#     git log --oneline private/todo2
#     git show private/todo2:todo2.txt          # newest version
#     git diff private/todo2~1 private/todo2    # what changed last snapshot
set -eu

BRANCH=private/todo2
FILE=todo2.txt

root=$(git rev-parse --show-toplevel)
cd "$root"

if [ ! -f "$FILE" ]; then
    echo "snapshot-todo2: no $FILE in $root — nothing to snapshot." >&2
    echo "  (it lives in the 'os' integration worktree; run this from there.)" >&2
    exit 1
fi

blob=$(git hash-object -w "$FILE")
tree=$(printf '100644 blob %s\t%s\n' "$blob" "$FILE" | git mktree)

if git rev-parse --verify -q "refs/heads/$BRANCH" >/dev/null; then
    prev_tree=$(git rev-parse "refs/heads/$BRANCH^{tree}")
    if [ "$prev_tree" = "$tree" ]; then
        echo "snapshot-todo2: unchanged since $(git log -1 --format=%cd --date=short "refs/heads/$BRANCH") — nothing to do."
        exit 0
    fi
    commit=$(git commit-tree "$tree" -p "refs/heads/$BRANCH" \
                 -m "todo2 snapshot $(date -u '+%Y-%m-%d %H:%M UTC')")
else
    commit=$(git commit-tree "$tree" \
                 -m "todo2 snapshot $(date -u '+%Y-%m-%d %H:%M UTC') (first)")
fi

git update-ref "refs/heads/$BRANCH" "$commit"
echo "snapshot-todo2: $BRANCH -> $(git rev-parse --short "$commit") ($(wc -c <"$FILE") bytes)"
