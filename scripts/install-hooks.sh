#!/bin/sh
# Install this repo's git hooks from scripts/hooks/ into the shared .git dir.
#
# All four worktrees (os, os-lane-a, os-lane-b, os-lane-c) share ONE repository,
# so `git rev-parse --git-common-dir` resolves to the same place from any of
# them and a single run here arms the hook for all three lane agents. Running it
# again from another worktree is harmless.
#
# Hooks are not carried by clone or fetch — .git/hooks is not part of the tree —
# so this has to be run once per clone. The sources are tracked under
# scripts/hooks/ precisely so they survive that.
set -eu

repo_root=$(git rev-parse --show-toplevel)
common=$(git rev-parse --git-common-dir)

# --git-common-dir can come back relative (".git"), and it is relative to the
# *current* directory, not the repo root — resolve it before we cd anywhere.
case "$common" in
    /* | ?:[\\/]*) ;;
    *) common="$(cd "$common" && pwd)" ;;
esac

src="$repo_root/scripts/hooks"
dst="$common/hooks"

mkdir -p "$dst"
for hook in "$src"/*; do
    [ -f "$hook" ] || continue
    name=$(basename "$hook")
    cp "$hook" "$dst/$name"
    chmod +x "$dst/$name"
    echo "installed $name -> $dst/$name"
done

echo "Done. Verify with: git config --get core.hooksPath  (should be unset)"
