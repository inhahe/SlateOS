#!/usr/bin/env bash
# Differential test: our `rmdir` against GNU coreutils 9.4's.
#
# Each case runs both sides in their own copy of one fixture tree and
# compares what they print, what they say, their status, and the tree they
# leave -- every path's type and mode -- since a directory removed that should
# have stayed, or left that should have gone, is the failure that matters.
#
# ## What it covers
#
#   * **the plain form** -- empty, not empty, missing, a file, `.`, `..`, the
#     root, a link to a directory (with and without a trailing slash, which
#     upstream words specially), a dangling link, several operands with a
#     failure among them.
#   * **`-p`** -- upstream's textual walk (`./cc` reaches `.` and fails there;
#     `a//b` reaches `a`), a non-empty ancestor, an ancestor in an unwritable
#     parent, a link among the ancestors.
#   * **`-v`** -- a line before every attempt, ancestors included.
#   * **`--ignore-fail-on-non-empty`** -- which failures count as "not empty":
#     `ENOTEMPTY` outright, and `EACCES` only when the directory really has an
#     entry.
#   * **the options** -- `--path` (the deprecated alias), abbreviations, the
#     ambiguous `--v`, and `POSIXLY_CORRECT` ending option parsing.
#   * **the descriptors** -- standard output full and closed with `-v`,
#     standard error full and closed, both into one file.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='rmdir'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "rmdir-diff: refusing to run as root, where the unwritable fixtures are" >&2
  echo "  writable and their cases measure nothing." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
CASE_ENV=

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -----------------------------------
proto=$work/proto
mkdir -p "$proto/empty" "$proto/full" "$proto/a/b/c" "$proto/x/y/z" "$proto/cc" \
         "$proto/ro/sub" "$proto/ro/emptysub" "$proto/target"
printf 'f\n' > "$proto/full/file"
printf 'k\n' > "$proto/x/keep"
printf 'f\n' > "$proto/file.txt"
printf 's\n' > "$proto/ro/sub/file"
ln -s target "$proto/link-to-dir"
ln -s nowhere "$proto/dangling"
ln -s file.txt "$proto/link-to-file"
mkdir -p "$proto/via/real/leaf"
ln -s real "$proto/via/link"
mkdir "$proto/$(printf 'new\nline')"
mkdir "$proto/$(printf 'bad\377name')"
chmod 555 "$proto/ro"

# --- comparing a whole tree ------------------------------------------------------
# Mode and type per path, sorted: `%A` is both at once. Each name goes through
# `od -c`, so one holding a newline or a byte that is not UTF-8 is compared
# exactly.
snap() {
  ( cd "$1" && find . -mindepth 1 -print0 | LC_ALL=C sort -z \
      | while IFS= read -r -d '' e; do
          printf '%s %s\n' "$(printf '%s' "$e" | od -An -c | tr -s ' \n' ' ')" \
            "$(stat -c '%A' "$e" 2>/dev/null || echo '?')"
        done )
}

# `run_side DIR SIDE REDIR BIN ERR ARGS...`: one side in its copy of the
# tree, with one descriptor as REDIR says. Standard error closed goes through
# a shell's `exec 2>&-`, so that only `rmdir` runs without it.
run_side() {
  local dir=$1 side=$2 redir=$3 bin=$4 err=$5; shift 5
  case $redir in
    '')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 ${CASE_ENV:+"$CASE_ENV"} \
          PATH="$bindir/$side" rmdir "$@" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" rmdir "$@" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" rmdir "$@" ) </dev/null >&- 2>"$err" ;;
    '<&-')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" rmdir "$@" ) <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" rmdir "$@" ) </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" /bin/sh -c 'exec 2>&-; exec rmdir "$@"' sh "$@" ) \
          </dev/null >"$bin" ;;
    # Both streams into one file: where a diagnostic lands among the `-v`
    # lines is upstream's `error ()` flushing standard output first.
    '2>&1')
      ( cd "$dir" && diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 \
          PATH="$bindir/$side" rmdir "$@" ) </dev/null >"$bin" 2>&1 ;;
    *) echo "rmdir-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local redir=$1; shift
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  run_side "$o_dir" ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side "$g_dir" gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg o_tree g_tree
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snap "$o_dir"); g_tree=$(snap "$g_dir")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  chmod -R u+rwx "$o_dir" "$g_dir" 2>/dev/null
  rm -rf "$o_dir" "$g_dir"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s {%s}\n  gnu  (rc=%s): %s {%s}\n  tree diff:\n%s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" \
    "$(diff <(printf '%s\n' "$o_tree") <(printf '%s\n' "$g_tree") | sed 's/^/    /' | head -8)")
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

run_case() { compare_with '' "$@"; report "${CASE_ENV:+$CASE_ENV }rmdir $*"; }

# `fd_case REDIR ARGS...`: run_case with one descriptor changed.
fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "rmdir $* $redir"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS rmdir %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail rmdir %s (%s)\n' "$*" "$why"
  fi
  return 0
}

nl_name=$(printf 'new\nline')
bad_utf8=$(printf 'bad\377name')

# --- the plain form ------------------------------------------------------------------
run_case empty
run_case empty/
run_case empty//
run_case full
run_case nosuch
run_case file.txt
run_case file.txt/
run_case ''
run_case .
run_case ..
run_case /
run_case empty/.
run_case empty/..
run_case link-to-dir
run_case link-to-dir/
run_case link-to-file/
run_case dangling
run_case dangling/
run_case nosuch empty full cc
run_case empty empty
run_case "$nl_name"
run_case "$bad_utf8"
run_case ro/emptysub
run_case ro/sub
run_case -- -p
run_case -

# --- -p ---------------------------------------------------------------------------------
run_case -p a/b/c
run_case -p a/b/c/
run_case -p a//b//c
run_case -p ./cc
run_case -p x/y/z
run_case -p a/b
run_case -p ro/emptysub
run_case -p via/link/leaf
run_case -p via/real/leaf
run_case -p empty a/b/c x/y/z
run_case -p full
run_case -p a/b/c a/b
run_case --parents a/b/c
run_case --path a/b/c
run_case --p a/b/c
run_case --pa a/b/c

# --- -v ---------------------------------------------------------------------------------
run_case -v empty
run_case -v empty full nosuch cc
run_case -pv a/b/c
run_case -pv x/y/z
run_case -pv ./cc
run_case -pv a//b//c/
run_case -v link-to-dir/
run_case -v "$nl_name"
run_case --verbose empty
run_case --verb empty

# --- --ignore-fail-on-non-empty ---------------------------------------------------------
run_case --ignore-fail-on-non-empty full
run_case --ignore-fail-on-non-empty empty full cc
run_case --ignore-fail-on-non-empty nosuch
run_case --ignore-fail-on-non-empty file.txt
run_case --ignore-fail-on-non-empty ro/sub
run_case --ignore-fail-on-non-empty ro/emptysub
run_case --ignore-fail-on-non-empty -p x/y/z
run_case --ignore-fail-on-non-empty -pv x/y/z
run_case --ignore-fail-on-non-empty -p ./cc
run_case --ignore-fail-on-non-empty -v full
run_case --i full
run_case --ignore full

# --- the options --------------------------------------------------------------------------
run_case
run_case -p
run_case -v
run_case --
run_case -q empty
run_case --nosuch empty
run_case --v empty
run_case --=x empty
run_case --parents=yes empty
run_case --ignore-fail-on-non-empty=1 full
run_case empty -p
CASE_ENV=POSIXLY_CORRECT=1 run_case empty -p
CASE_ENV=POSIXLY_CORRECT=1 run_case -p a/b/c
run_case --nosuch --help

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' empty
fd_case '>/dev/full' -v empty
fd_case '>/dev/full' -pv a/b/c
fd_case '>&-' empty
fd_case '>&-' -v empty
fd_case '>&-' -v nosuch
fd_case '<&-' empty
fd_case '2>/dev/full' empty
fd_case '2>/dev/full' full
fd_case '2>/dev/full' full empty
fd_case '2>/dev/full' -q
fd_case '2>/dev/full' --ignore-fail-on-non-empty full
fd_case '2>&-' empty
fd_case '2>&-' full
fd_case '2>&-' -v full empty
fd_case '2>&1' -v empty full cc
fd_case '2>&1' -pv x/y/z
fd_case '2>&1' -v nosuch empty
fd_case '>/dev/full' --help
fd_case '>&-' --help
fd_case '>/dev/full' --version
fd_case '>&-' --version

# --- the ones whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version
xfail_case "our help text: --help is acted on before the bad option" --help --nosuch
xfail_case "our version string: the first of the two wins" --version --help

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
