#!/usr/bin/env bash
# Differential test: our `chgrp` against GNU `chgrp`.
#
# ## Built like `chown-diff.sh`, and why
#
# `chgrp`'s result is a changed inode, not output, so each case runs in two
# disposable copies of one tree and what is compared afterwards is every path's
# owner and group, beside stdout, stderr and the status. Unprivileged, a change
# to a group the invoking user belongs to is a REAL change and a change to any
# other group is a refusal with a diagnostic, so both kinds of case exist
# without root -- and the harness refuses to run as root, where every refusal
# below would become a change.
#
# ## What this is checking
#
#   * **the group operand** -- a name before a number, `xstrtoumax`'s reading
#     of a number (` 5` and `+5` are 5; `5 `, `0x5`, `-1` are invalid),
#     `4294967295` as "unchanged", the empty operand, `--reference`.
#   * **the walk, which is `chown`'s** (`coreutils::chowncore`) -- post-order
#     under `-R`, and the two settings the symlink options reduce to: which
#     links the walk goes through (`-P`/`-H`/`-L`) and whether a link met is
#     changed or its target is (`--dereference`/`-h`). The fixture has links
#     inside the tree and a loop back to an ancestor, which is what tells
#     those settings apart: under `-R -H` a link inside the tree has its
#     TARGET changed, under `-R -L -h` a link is changed while walked through,
#     and `-R --dereference` without `-H`/`-L` is refused.
#   * **reporting** -- `-v`/`-c` wording and order, `-f`, a full stdout.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS. `-v` on
# a symlink whose target cannot be looked up: upstream prints its `from` half
# out of a stat buffer the failed call never filled, which differs from run to
# run (see `chowncore.rs`); ours prints no `from`.
set -u

DIFF_PROG='chgrp'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "chgrp-diff: refusing to run as root: every refusal below would become" >&2
  echo "  a change, and a mistake in the fixtures would have privilege behind it." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

ME=$(id -un)
MYGROUP=$(id -gn)
# A group the invoking user belongs to that is NOT the primary one: a change to
# it is permitted and observable. Refuse rather than report a green run that
# measured only diagnostics.
ALTGROUP=$(id -Gn | tr ' ' '\n' | grep -v "^$MYGROUP$" | head -1)
if [ -z "$ALTGROUP" ]; then
  echo "chgrp-diff: $ME belongs to only one group, so no change is permitted" >&2
  echo "  here and every case would be a refusal. Skipping." >&2
  exit 0
fi
ALTGID=$(getent group "$ALTGROUP" | cut -d: -f3)

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -------------------------------------
proto=$work/proto
mkdir -p "$proto/dir/sub" "$proto/dir2"
printf 'a\n' > "$proto/file.txt"
printf 'b\n' > "$proto/dir/inner.txt"
printf 'c\n' > "$proto/dir/sub/deep.txt"
printf 'd\n' > "$proto/dir2/other.txt"
# Links, all relative and all inside the tree.
ln -s file.txt "$proto/link-to-file"
ln -s dir "$proto/link-to-dir"
ln -s nowhere "$proto/dangling"
# Inside a directory, so that `-R` meets them rather than being handed them.
ln -s ../file.txt "$proto/dir/in-link"
ln -s ../dir2 "$proto/dir/in-dirlink"
# Back to an ancestor: a loop, for `-L`.
ln -s .. "$proto/dir/sub/up"

escaped=$(cd "$proto" && find . -type l -print | while IFS= read -r l; do
  t=$(readlink -f "$l" 2>/dev/null) || continue
  case "$t" in
    "$proto"/*|"$proto") ;;
    *) printf '%s -> %s\n' "$l" "$t" ;;
  esac
done)
if [ -n "$escaped" ]; then
  echo "chgrp-diff: a fixture symlink resolves outside the fixture tree:" >&2
  printf '  %s\n' "$escaped" >&2
  echo "  Refusing: 'chgrp -R -L' would follow it and change something real." >&2
  exit 2
fi

# Owner and group of every path, the links themselves included (`stat` does not
# follow), sorted.
snap() {
  ( cd "$1" && find . -mindepth 1 -print | LC_ALL=C sort \
      | while IFS= read -r e; do
          printf '%s %s\n' "$e" "$(stat -c '%U:%G' "$e" 2>/dev/null || echo '?:?')"
        done )
}

run_side() {
  local dir=$1 side=$2; shift 2
  ( cd "$dir" && diff_run timeout -k 2 20 \
      env LC_ALL=C.UTF-8 PATH="$bindir/$side" chgrp "$@" )
}

compare() {
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ -n "$TO_FULL" ]; then
    run_side "$o_dir" ours "$@" </dev/null >/dev/full 2>"$o_err"; o_rc=$?
    run_side "$g_dir" gnu  "$@" </dev/null >/dev/full 2>"$g_err"; g_rc=$?
  else
    run_side "$o_dir" ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg o_tree g_tree
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snap "$o_dir"); g_tree=$(snap "$g_dir")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  rm -rf "$o_dir" "$g_dir"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s {%s}\n    tree: %s\n  gnu  (rc=%s): %s {%s}\n    tree: %s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$(printf '%s' "$o_tree" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" \
    "$(printf '%s' "$g_tree" | tr '\n' '|')")
}

run_case() {
  local label="chgrp $*${TO_FULL:+  [>/dev/full]}"
  compare "$@"
  TO_FULL=
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS chgrp %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail chgrp %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the group operand -----------------------------------------------------------------
run_case "$ALTGROUP" file.txt
run_case "$ALTGID" file.txt
run_case " $ALTGID" file.txt
run_case "+$ALTGID" file.txt
run_case "$ALTGID " file.txt
run_case 0x10 file.txt
run_case -- -1 file.txt
run_case 4294967295 file.txt
run_case 4294967296 file.txt
run_case 99999999999999999999 file.txt
run_case nosuchgroup file.txt
run_case '' file.txt
run_case "$MYGROUP" file.txt
run_case root file.txt
run_case 0 file.txt
run_case "$ALTGROUP" nosuchfile.txt
run_case "$ALTGROUP" file.txt nosuchfile.txt dir

# --- files, links, directories ------------------------------------------------------
run_case "$ALTGROUP" dir
run_case "$ALTGROUP" link-to-file
run_case -h "$ALTGROUP" link-to-file
run_case --no-dereference "$ALTGROUP" link-to-file
run_case --dereference "$ALTGROUP" link-to-file
run_case --dereference -h "$ALTGROUP" link-to-file
run_case -h --dereference "$ALTGROUP" link-to-file
run_case "$ALTGROUP" dangling
run_case -h "$ALTGROUP" dangling
run_case "$ALTGROUP" link-to-dir
run_case -h "$ALTGROUP" link-to-dir

# --- recursion, and the two settings it reduces the symlink options to -----------------
run_case -R "$ALTGROUP" dir
run_case -R "$ALTGROUP" .
run_case -R -P "$ALTGROUP" .
run_case -R -h "$ALTGROUP" .
run_case -R -H "$ALTGROUP" .
run_case -R -H -h "$ALTGROUP" .
run_case -R -H --dereference "$ALTGROUP" .
run_case -R -L "$ALTGROUP" .
run_case -R -L -h "$ALTGROUP" .
run_case -R -H "$ALTGROUP" link-to-dir
run_case -R -L "$ALTGROUP" link-to-dir
run_case -R -P "$ALTGROUP" link-to-dir
run_case -R -H -P "$ALTGROUP" link-to-dir
run_case -R -P -L "$ALTGROUP" link-to-dir
run_case -R --dereference "$ALTGROUP" dir
run_case -R --dereference
run_case -R -P --dereference "$ALTGROUP" dir
run_case -R root dir
run_case -R -f root dir

# --- --reference ---------------------------------------------------------------------
run_case --reference=dir/inner.txt file.txt
run_case --reference=link-to-file dir
run_case --reference=nosuchfile.txt file.txt
run_case --reference=dangling file.txt
run_case --reference=file.txt -R dir

# --- reporting -------------------------------------------------------------------------
run_case -v "$ALTGROUP" file.txt
run_case -c "$ALTGROUP" file.txt
run_case -v "$MYGROUP" file.txt
run_case -c "$MYGROUP" file.txt
run_case -v "$ALTGID" file.txt
run_case -v '' file.txt
run_case -v root file.txt
run_case -v "$ALTGROUP" nosuchfile.txt
run_case -v -h "$ALTGROUP" link-to-file
run_case -v "$ALTGROUP" link-to-file
run_case -c "$ALTGROUP" dangling
run_case -Rv "$ALTGROUP" dir
run_case -Rc "$ALTGROUP" .
run_case -R -L -v "$ALTGROUP" dir
run_case -R -H -v "$ALTGROUP" dir
run_case -v --reference=dir/inner.txt file.txt
run_case -f root file.txt
run_case --silent root file.txt
run_case --quiet nosuchgroup file.txt
run_case -f "$ALTGROUP" nosuchfile.txt
xfail_case "upstream's from-half is uninitialised memory" -v "$ALTGROUP" dangling

# --- a full stdout ---------------------------------------------------------------------
TO_FULL=1; run_case -v "$ALTGROUP" file.txt
TO_FULL=1; run_case "$ALTGROUP" file.txt

# --- the command line -------------------------------------------------------------------
run_case
run_case "$ALTGROUP"
run_case --reference=file.txt
run_case -Q "$ALTGROUP" file.txt
run_case --nosuchoption "$ALTGROUP" file.txt
run_case --from=x "$ALTGROUP" file.txt
run_case --recur "$ALTGROUP" dir
run_case --no-deref "$ALTGROUP" link-to-file
run_case --ref=dir/inner.txt file.txt
run_case --chang "$ALTGROUP" file.txt
run_case --s root file.txt
run_case --no "$ALTGROUP" file.txt
run_case --verbose=1 "$ALTGROUP" file.txt
run_case -- "$ALTGROUP" file.txt
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
