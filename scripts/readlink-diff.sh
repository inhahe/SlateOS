#!/usr/bin/env bash
# Differential test: our `readlink` against GNU coreutils 9.4's.
#
# `readlink` prints a link's target, or with `-f`, `-e` or `-m` a name's
# canonical form -- an absolute path. So the two sides cannot run in two
# copies of the fixture side by side, as the harnesses that compare trees do:
# their answers would differ by the copy's name. Each case instead runs both
# sides one after the other in the *same* directory, rebuilt between them, and
# compares what they printed (as bytes), what they said, and their status.
#
# ## What it covers
#
#   * **reading a link** -- relative and absolute targets, a chain (only one
#     step is read), a target with a newline or a byte that is not UTF-8, a
#     name that is not a link, missing, a directory, a trailing slash.
#   * **the three canonical modes** -- `-f` (all but the last component must
#     exist), `-e` (all must), `-m` (none need): dangling links, loops, `..`
#     after a link, a file in the middle of the name, `/`, `.`, `//`, the
#     empty name.
#   * **the output options** -- `-n` (refused, with a warning, for more than
#     one operand) and `-z`; `-q`, `-s` and `-v`, last one wins.
#   * **the descriptors** -- standard output full and closed, standard error
#     full and closed, standard input closed.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='readlink'
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
  mkdir -p "$case_dir/dir/sub" "$case_dir/loopdir"
  printf 'a\n' > "$case_dir/file.txt"
  printf 'b\n' > "$case_dir/dir/inner.txt"
  ln -s file.txt "$case_dir/rel"
  ln -s "$case_dir/file.txt" "$case_dir/abs"
  ln -s rel "$case_dir/chain"
  ln -s nowhere "$case_dir/dangling"
  ln -s nowhere/deeper "$case_dir/dangling-deep"
  ln -s dir "$case_dir/link-to-dir"
  ln -s dir/sub "$case_dir/link-to-sub"
  ln -s self "$case_dir/self"
  ln -s loop-b "$case_dir/loop-a"
  ln -s loop-a "$case_dir/loop-b"
  ln -s ../file.txt "$case_dir/dir/up"
  ln -s file.txt/x "$case_dir/through-file"
  ln -s / "$case_dir/to-root"
  ln -s "" "$case_dir/empty-target" 2>/dev/null || true
  ln -s "$(printf 'new\nline')" "$case_dir/nl-target"
  ln -s "$(printf 'bad\377byte')" "$case_dir/bad-target"
  ln -s file.txt "$case_dir/$(printf 'nl\nname')"
  ln -s ../.. "$case_dir/dir/sub/upup"
}

# `run_side SIDE REDIR BIN ERR ARGS...`: one side in the case directory, with
# one descriptor as REDIR says.
run_side() {
  local side=$1 redir=$2 bin=$3 err=$4; shift 4
  case $redir in
    '')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) </dev/null >&- 2>"$err" ;;
    '<&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" /bin/sh -c 'exec 2>&-; exec readlink "$@"' sh "$@" ) \
          </dev/null >"$bin" ;;
    '2>&1')
      ( cd "$case_dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" readlink "$@" ) </dev/null >"$bin" 2>&1 ;;
    *) echo "readlink-diff: no such redirection: $redir" >&2; exit 2 ;;
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

run_case() { compare_with '' "$@"; report "readlink $*"; }

fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "readlink $* $redir"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS readlink %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail readlink %s (%s)\n' "$*" "$why"
  fi
  return 0
}

nl_name=$(printf 'nl\nname')

# --- reading a link ------------------------------------------------------------------
for f in rel abs chain dangling dangling-deep link-to-dir self loop-a dir/up through-file \
         to-root nl-target bad-target "$nl_name" file.txt dir nosuch rel/ link-to-dir/ \
         dir/sub/upup '' . / empty-target; do
  run_case "$f"
  run_case -v "$f"
done
run_case rel abs nosuch chain
run_case -v rel nosuch file.txt abs

# --- the three canonical modes --------------------------------------------------------
for mode in -f -e -m; do
  for f in rel abs chain dangling dangling-deep link-to-dir link-to-sub self loop-a \
           dir/up through-file to-root nl-target bad-target file.txt dir nosuch \
           nosuch/deeper file.txt/x file.txt/ dir/ dir/.. link-to-dir/.. link-to-sub/.. \
           dir/sub/upup dir/sub/upup/file.txt . .. / // /// ./. '' rel/ dangling/ \
           link-to-dir/inner.txt loop-a/x nosuch/../file.txt "$nl_name"; do
    run_case "$mode" "$f"
  done
  run_case "$mode" -v nosuch/deeper
  run_case "$mode" -v loop-a
  run_case "$mode" -v file.txt/x
  run_case "$mode" rel nosuch/deeper abs
done
run_case --canonicalize rel
run_case --canonicalize-existing rel
run_case --canonicalize-missing nosuch/x
run_case --canon rel
run_case -f -e nosuch
run_case -e -f nosuch
run_case -m -f nosuch/x

# --- the output options ----------------------------------------------------------------
run_case -n rel
run_case -n rel abs
run_case -n -q rel abs
run_case -n -f rel abs
run_case -z rel
run_case -z rel abs chain
run_case -z -n rel
run_case -zn rel abs
run_case -fz rel nosuch abs
run_case --no-newline rel
run_case --zero rel abs
run_case -q nosuch
run_case -s nosuch
run_case -v nosuch
run_case -v -q nosuch
run_case -q -v nosuch
run_case -s -v nosuch
run_case --quiet -v nosuch
run_case --silent nosuch
run_case --verbose nosuch
run_case -v -f nosuch/x

# --- the options --------------------------------------------------------------------------
run_case
run_case -f
run_case -v
run_case --
run_case -- -rel
run_case -x rel
run_case --nosuch rel
run_case --canonicalize=yes rel
run_case --c rel
run_case --q rel
run_case --s rel
run_case --v rel
run_case --=x rel
run_case --nosuch --help

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' rel
fd_case '>/dev/full' -f rel
fd_case '>/dev/full' nosuch
fd_case '>/dev/full' -n rel
fd_case '>&-' rel
fd_case '>&-' nosuch
fd_case '>&-' -v nosuch
fd_case '<&-' rel
fd_case '2>/dev/full' rel
fd_case '2>/dev/full' -v nosuch
fd_case '2>/dev/full' nosuch
fd_case '2>/dev/full' -n rel abs
fd_case '2>/dev/full' -x
fd_case '2>&-' -v nosuch
fd_case '2>&-' rel
fd_case '2>&-' -n rel abs
fd_case '2>&1' -v rel nosuch abs
fd_case '2>&1' -n rel abs
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
