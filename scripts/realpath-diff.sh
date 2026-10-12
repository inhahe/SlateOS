#!/usr/bin/env bash
# Differential test: our `realpath` against GNU coreutils 9.4's.
#
# `realpath` prints absolute paths, so -- as in `readlink-diff.sh` -- the two
# sides cannot run in two copies of the fixture side by side: their answers
# would differ by the copy's name. Each case runs both sides one after the
# other in the *same* directory, rebuilt between them, and compares what they
# printed (as bytes), what they said, and their status.
#
# ## What it covers
#
#   * **the three modes** -- the default (all but the last component must
#     exist), `-e` (all must), `-m` (none need): dangling links, loops, a file
#     in the middle of a name, `.`, `..`, `/`, `//`, the empty name.
#   * **how links are taken** -- `-P` (resolved as met, the default), `-L`
#     (`..` taken off before the link it follows is resolved), `-s` (not
#     resolved at all), last one wins.
#   * **`--relative-to` and `--relative-base`** -- alone and together, inside,
#     outside and equal to the base, through links, with `-m` and a missing
#     base.
#   * **the output** -- `-z`, `-q`, several operands with a failure among them.
#   * **the descriptors** -- standard output full and closed, standard error
#     full and closed, standard input closed.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='realpath'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built afresh for every run ------------------------------------------
case_dir=$work/case
build_tree() {
  rm -rf "$case_dir"
  mkdir -p "$case_dir/dir/sub/deep" "$case_dir/other"
  printf 'a\n' > "$case_dir/file.txt"
  printf 'b\n' > "$case_dir/dir/inner.txt"
  ln -s file.txt "$case_dir/rel"
  ln -s "$case_dir/file.txt" "$case_dir/abs"
  ln -s rel "$case_dir/chain"
  ln -s nowhere "$case_dir/dangling"
  ln -s dir/sub "$case_dir/link-to-sub"
  ln -s dir "$case_dir/link-to-dir"
  ln -s loop-b "$case_dir/loop-a"
  ln -s loop-a "$case_dir/loop-b"
  ln -s ../other "$case_dir/dir/to-other"
  ln -s / "$case_dir/to-root"
  ln -s file.txt "$case_dir/$(printf 'nl\nname')"
  mkdir "$case_dir/$(printf 'bad\377dir')"
}

# `run_side SIDE REDIR BIN ERR ARGS...`: one side in the case directory, with
# one descriptor as REDIR says.
run_side() {
  local side=$1 redir=$2 bin=$3 err=$4; shift 4
  case $redir in
    '')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) </dev/null >&- 2>"$err" ;;
    '<&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" /bin/sh -c 'exec 2>&-; exec realpath "$@"' sh "$@" ) \
          </dev/null >"$bin" ;;
    '2>&1')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" realpath "$@" ) </dev/null >"$bin" 2>&1 ;;
    *) echo "realpath-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local redir=$1; shift
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  build_tree
  run_side ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?
  build_tree
  run_side gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s {%s}\n  gnu  (rc=%s): %s {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare_with '' "$@"; report "realpath $*"; }

fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "realpath $* $redir"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS realpath %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail realpath %s (%s)\n' "$*" "$why"
  fi
  return 0
}

nl_name=$(printf 'nl\nname')
bad_dir=$(printf 'bad\377dir')

# --- the three modes ----------------------------------------------------------------------
for mode in '' -e -m; do
  for f in rel abs chain dangling link-to-sub link-to-dir loop-a dir/to-other to-root \
           file.txt dir nosuch nosuch/deeper file.txt/x file.txt/ dir/ dir/.. \
           link-to-sub/.. link-to-sub/../inner.txt . .. / // /// ./. '' rel/ dangling/ \
           loop-a/x nosuch/../file.txt "$nl_name" "$bad_dir" "$bad_dir/x"; do
    # shellcheck disable=SC2086  # an empty mode is no argument at all
    run_case $mode "$f"
  done
done
run_case rel nosuch/deeper abs
run_case -e rel nosuch abs
run_case --canonicalize-existing rel
run_case --canonicalize-missing nosuch/x
run_case -e -m nosuch/x
run_case -m -e nosuch/x

# --- how links are taken ---------------------------------------------------------------
for links in -P -L -s --strip --no-symlinks --logical --physical; do
  for f in rel chain link-to-sub/.. link-to-sub/../inner.txt dir/to-other/.. dangling \
           loop-a nosuch/.. dir/sub/../.. link-to-dir/sub/..; do
    run_case "$links" "$f"
  done
done
run_case -L -P link-to-sub/..
run_case -P -L link-to-sub/..
run_case -s -L link-to-sub/..
run_case -L -s link-to-sub/..
run_case -m -s nosuch/../x
run_case -e -L link-to-sub/../inner.txt

# --- relative output ---------------------------------------------------------------------
for f in file.txt dir/inner.txt dir/sub/deep other . / rel link-to-sub; do
  run_case --relative-to=dir "$f"
  run_case --relative-to=dir/sub/deep "$f"
  run_case --relative-to=/ "$f"
  run_case --relative-base=dir "$f"
  run_case --relative-base=. "$f"
  run_case --relative-to=dir --relative-base=. "$f"
  run_case --relative-to=dir/sub --relative-base=dir "$f"
done
run_case --relative-to=nosuch file.txt
run_case -m --relative-to=nosuch file.txt
run_case --relative-to=file.txt dir
run_case --relative-to=link-to-sub dir/inner.txt
run_case -s --relative-to=link-to-sub dir/inner.txt
run_case --relative-to dir file.txt
run_case --relative-to= file.txt
run_case --relative-base= file.txt
run_case --relative-to=dir --relative-to=other file.txt
run_case --relative-base=dir/sub --relative-to=dir file.txt

# --- the output --------------------------------------------------------------------------
run_case -z rel abs
run_case -z rel nosuch/x abs
run_case --zero rel
run_case -q nosuch/x
run_case -q rel nosuch/x abs
run_case --quiet nosuch/x
run_case -e -q nosuch

# --- the options --------------------------------------------------------------------------
run_case
run_case -e
run_case --
run_case -- -rel
run_case -x rel
run_case --nosuch rel
run_case --relative-to
run_case --strip=yes rel
run_case --c rel
run_case --canonicalize-e rel
run_case --rel=dir file.txt
run_case --relative-t=dir file.txt
run_case --=x rel
run_case --nosuch --help

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' rel
fd_case '>/dev/full' nosuch/x
fd_case '>&-' rel
fd_case '>&-' nosuch/x
fd_case '<&-' rel
fd_case '2>/dev/full' rel
fd_case '2>/dev/full' nosuch/x
fd_case '2>/dev/full' rel nosuch/x
fd_case '2>/dev/full' -x
fd_case '2>&-' nosuch/x
fd_case '2>&-' rel
fd_case '2>&1' rel nosuch/x abs
fd_case '>/dev/full' --help
fd_case '>&-' --help
fd_case '>/dev/full' --version
fd_case '>&-' --version

# --- the ones whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version
xfail_case "our help text: --help is acted on before the bad option" --help --nosuch
xfail_case "our version string: the first of the two wins" --version --help

rm -rf "$case_dir"
printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
