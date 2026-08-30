#!/bin/sh
# Install this repo's git hooks into the shared .git dir, as trampolines.
#
# All four worktrees (os, os-lane-a, os-lane-b, os-lane-c) share ONE repository,
# so `git rev-parse --git-common-dir` resolves to the same place from any of
# them and a single run here arms the hooks for all three lane agents. Running
# it again from another worktree is harmless.
#
# Hooks are not carried by clone or fetch — .git/hooks is not part of the tree —
# so this has to be run once per clone. The sources are tracked under
# scripts/hooks/ precisely so they survive that.
#
# ## Why a trampoline and not a copy
#
# This used to `cp` each hook into place, and that is wrong in two ways that
# both fail silently.
#
# **An edit to the tracked hook did nothing until someone remembered to re-run
# this script.** On 2026-08-30 a new gate was added to scripts/hooks/pre-push,
# committed, and pushed — and the push it was meant to check ran the copy
# installed weeks earlier. The output looked entirely normal: every other gate
# reported in, and the new one simply was not there. That is the worst shape a
# guard can fail in, because "the gate found nothing" and "the gate does not
# exist" print the same thing, which is nothing.
#
# **One installed copy cannot serve four worktrees at different commits.** The
# hook runs scripts/*.py out of `git rev-parse --show-toplevel`, so a copy
# installed from lane-b's tree was already running lane-a's checkers whenever
# lane A pushed. Content and caller came from different commits — and a copy
# reinstalled by whichever lane ran this script last would silently downgrade
# the gates for the other two.
#
# The trampoline fixes both at once: it resolves the working tree at push time
# and execs that tree's own tracked hook, so the hook and the scripts it calls
# always come from one commit, each lane always runs the version it has checked
# out, and an edit takes effect the moment it is saved. The trampoline body has
# no version of its own to drift — it names the hook by `$0`'s basename, so it
# is the same eight lines for every hook, and installing it is a one-time act.
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

    # `exec` keeps stdin: git feeds pre-push its ref list that way, and a
    # trampoline that swallowed it would make every hook see an empty push.
    #
    # A missing tracked hook is a skip, not a failure. Checking out a commit
    # from before the hook existed — a bisect, an archaeology dig — must not
    # make the tree unpushable, and the note on stderr says why nothing ran.
    #
    # SLATEOS_HOOK_TRAMPOLINE tells the hook it was reached the supported way.
    # A hook that finds it unset was installed as a copy, which is the failure
    # this file exists to end, and it says so rather than running on quietly.
    cat > "$dst/$name" <<'TRAMPOLINE'
#!/bin/sh
# Installed by scripts/install-hooks.sh. Do not edit: this is a trampoline to
# the tracked hook of whichever worktree the command was run from. The hook
# itself lives at scripts/hooks/<name> and is the file to change.
set -eu
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
real="$root/scripts/hooks/${0##*/}"
if [ ! -f "$real" ]; then
    echo "hook ${0##*/}: $real does not exist in this checkout; nothing ran" >&2
    exit 0
fi
SLATEOS_HOOK_TRAMPOLINE=1
export SLATEOS_HOOK_TRAMPOLINE
exec sh "$real" "$@"
TRAMPOLINE
    chmod +x "$dst/$name"
    echo "installed $name -> $dst/$name (trampoline to scripts/hooks/$name)"
done

echo "Done. Verify with: git config --get core.hooksPath  (should be unset)"
