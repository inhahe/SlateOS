#!/usr/bin/env bash
# unlink-diff.sh — compare our `unlink` and `link` against GNU's, inside WSL.
#
# ## What this is checking
#
# Each program makes one system call and reports its one answer, so there are
# three things to agree on: the diagnostic (and its quoting), the status, and
# the filesystem afterwards. The third is why every case runs in a fresh copy
# of one fixture directory per side, and why the comparison includes a listing
# of that directory once the program has run -- type, link count and symlink
# target of everything in it. A `link` that exits 0 having linked the wrong
# thing (say, the target of a symlink instead of the symlink) agrees on
# everything but that listing.
#
#   * **unlink** -- a file, a symlink (the link goes, not its target), a
#     dangling symlink, a missing name, a directory (`Is a directory`), the
#     empty name, a trailing slash on a file, a name that needs quoting, and
#     a directory that will not let us remove anything.
#   * **link** -- a file, an existing new name, a missing old one, a
#     directory (`Operation not permitted`), a symlink (linked, not followed).
#   * **the command line** -- missing and extra operands (which one is named),
#     `--` and `-` as operands, an unknown option, and the one `getopt_long`
#     call that lets an option anywhere on the line decide.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='unlink'
DIFF_BINS='unlink link'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# The fixture every case starts from. Built once per side per case, so one
# case's changes can never be the next one's starting point.
make_fixture() {
  local d=$1
  mkdir -p "$d"
  ( cd "$d" &&
    printf 'data\n' > f &&
    printf 'other\n' > g &&
    mkdir d &&
    printf 'inner\n' > d/x &&
    ln -s f sym &&
    ln -s nowhere dangling &&
    printf 'q\n' > 'has space' &&
    printf 'n\n' > "$(printf 'new\nline')" &&
    printf 'h\n' > -f &&
    mkdir ro && printf 'kept\n' > ro/k && chmod 555 ro )
}

# Everything in the directory, one line each: path, type, link count, and a
# symlink's target. Names are escaped by `%p`'s own rendering -- `find` prints
# a newline in a name literally, so the listing goes through `od -c`.
listing() {
  ( cd "$1" && find . -mindepth 1 -printf '%p %y %n %l\n' | LC_ALL=C sort | od -An -c )
}

compare() {
  local prog=$1; shift
  local side dir out err rc o_state g_state o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    dir=$DIFF_TMP/case-$side
    chmod -R u+w "$dir" 2>/dev/null; rm -rf "$dir"
    make_fixture "$dir"
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >"$out" 2>"$err"
    rc=$?
    if [ "$side" = ours ]; then
      o_rc=$rc; o_state=$(listing "$dir")
      o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_state=$(listing "$dir")
      g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$o_state" = "$g_state" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s}\n  gnu  (rc=%s): err{%s}\n  state %s' \
    "$o_rc" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$([ "$o_state" = "$g_state" ] && echo agrees || echo DIFFERS)")
}

run_case() {
  local label="$*"
  compare "$@"
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
  local label="$*"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- unlink: the one call -----------------------------------------------------
run_case unlink f
run_case unlink sym
run_case unlink dangling
run_case unlink missing
run_case unlink d
run_case unlink d/x
run_case unlink ''
run_case unlink f/
run_case unlink 'has space'
run_case unlink "$(printf 'new\nline')"
run_case unlink ro/k
run_case unlink -- -f
run_case unlink -

# --- unlink: the command line -----------------------------------------------
run_case unlink
run_case unlink f g
run_case unlink f g d
run_case unlink -x
run_case unlink --nope
run_case unlink f -x
run_case unlink --help=1
run_case unlink --
run_case unlink -- f g
xfail_case 'our --help omits the GNU ancillary block' unlink --help
xfail_case 'our --help omits the GNU ancillary block' unlink f --help
xfail_case 'our --version names SlateOS' unlink --version
xfail_case 'our --version names SlateOS' unlink --vers

# --- link: the one call -------------------------------------------------------
run_case link f new
run_case link f g
run_case link missing new
run_case link d new
run_case link sym new
run_case link dangling new
run_case link f d
run_case link f d/new
run_case link f missingdir/new
run_case link f ro/new
run_case link '' new
run_case link f ''
run_case link 'has space' 'also space'
run_case link -- -f new

# --- link: the command line -------------------------------------------------
run_case link
run_case link f
run_case link f g h
run_case link f g h i
run_case link -x
run_case link f new -x
run_case link --version=2
xfail_case 'our --help omits the GNU ancillary block' link --help
xfail_case 'our --version names SlateOS' link f new --version

# Restore write permission so the scratch area can be removed.
chmod -R u+w "$DIFF_TMP" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
