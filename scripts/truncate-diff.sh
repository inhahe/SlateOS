#!/usr/bin/env bash
# truncate-diff.sh — compare our `truncate` against the real GNU one, in WSL.
#
# ## What this is checking
#
# The effect of `truncate` is a file's size, so every case runs in a fresh
# fixture per side and the comparison includes a listing of the directory
# afterwards -- every name, its type and its size -- beside the output, the
# diagnostics and the status.
#
#   * **the SIZE grammar** -- absolute, `+`/`-`, `<`/`>`, `/`/`%`, whitespace
#     around the modifier, dd's suffixes in both families (`K` 1024, `KB`
#     1000, `KiB` 1024), a bad number, and a number too large to parse.
#   * **the quirks** -- the relative mode outliving its `-s` (`-s +5 -s 3`),
#     two modifiers, `division by zero` before anything else, the four
#     argument checks in upstream's order.
#   * **opening** -- a new file (created), `-c` on a missing file (quiet) and
#     on a missing directory component (also quiet: the errno is the same), a
#     directory, a read-only file, a FIFO with no reader (non-blocking, so
#     `ENXIO` rather than a hang), carrying on past a failure.
#   * **`-r` and `-o`** -- a reference size, a reference plus relative size, a
#     missing reference, and IO blocks (the fixture's `st_blksize`, whatever
#     the filesystem reports, on both sides).
#   * **the limits** -- a size `ftruncate` refuses, a block count that
#     overflows, an extension that overflows.
#
# Runs unprivileged, so a read-only file really is.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='truncate'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

make_fixture() {
  local d=$1
  mkdir -p "$d"
  ( cd "$d" &&
    printf '0123456789' > f &&
    head -c 100 /dev/zero > big &&
    : > empty &&
    head -c 37 /dev/zero > ref &&
    mkdir dir &&
    printf 'ro\n' > ro && chmod 444 ro &&
    mkfifo fifo &&
    printf 'g\n' > g )
}

listing() {
  ( cd "$1" && find . -mindepth 1 -maxdepth 2 -printf '%p %y %s\n' | LC_ALL=C sort )
}

compare() {
  local side dir out err rc o_out g_out o_err g_err o_rc g_rc o_state g_state
  for side in ours gnu; do
    dir=$DIFF_TMP/case-$side
    chmod -R u+rw "$dir" 2>/dev/null; rm -rf "$dir"
    make_fixture "$dir"
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" truncate "$@" ) >"$out" 2>"$err"
    rc=$?
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err"); o_state=$(listing "$dir")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err"); g_state=$(listing "$dir")
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$o_state" = "$g_state" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s}\n  gnu  (rc=%s): err{%s}\n  state %s\n%s\n---\n%s' \
    "$o_rc" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$([ "$o_state" = "$g_state" ] && echo agrees || echo DIFFERS)" \
    "$([ "$o_state" = "$g_state" ] || printf '%s' "$o_state")" \
    "$([ "$o_state" = "$g_state" ] || printf '%s' "$g_state")")
}

run_case() {
  local label="truncate $*"
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
  local label="truncate $*"
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

# --- the SIZE grammar --------------------------------------------------------
run_case -s 0 f
run_case -s 5 f
run_case -s 100 f
run_case -s +5 f
run_case -s -3 f
run_case -s -100 f
run_case -s '<5' big
run_case -s '<500' big
run_case -s '>200' f
run_case -s '>2' f
run_case -s /7 big
run_case -s %7 big
run_case -s %10 big
run_case -s ' > 50' f
run_case -s ' +  5' f
run_case -s 1K new
run_case -s 1KB new
run_case -s 1KiB new
run_case -s 2k new
run_case -s 1M new
run_case -s 1m new
run_case -s K new
run_case -s 1x f
run_case -s abc f
run_case -s '' f
run_case -s 99999999999999999999 f
run_case --size=5 f
run_case --size 5 f
run_case --si=5 f

# --- the quirks --------------------------------------------------------------
run_case -s +5 -s 3 f
run_case -s +5 -s +3 f
run_case -s '<+3' f
run_case -s /0 f
run_case -s %0
run_case -s /0
run_case f
run_case -r ref -s 5 f
run_case -o -r ref f
run_case -s 5
run_case -r ref
run_case

# --- opening -----------------------------------------------------------------
run_case -s 3 new
run_case -c -s 0 missing
run_case --no-create -s 0 missing
run_case -c -s 0 nodir/f
run_case -s 0 nodir/f
run_case -s 0 dir
run_case -s 0 dir/
run_case -s 0 ro
run_case -s 0 fifo
run_case -s 4 f missing ro g
run_case -s 0 ''

# --- -r and -o ---------------------------------------------------------------
run_case -r ref f
run_case -r ref -s +3 f
run_case -r ref -s '<10' f
run_case -r ref -s %16 new
run_case --reference=ref f
run_case -r missing f
run_case -r dir f
run_case -o -s 2 f
run_case -o -s +1 big
run_case --io-blocks -s 0 f

# --- the limits --------------------------------------------------------------
run_case -s 9223372036854775807 f
run_case -o -s 9223372036854775807 f
run_case -s +9223372036854775807 big
run_case -s -9223372036854775808 f

# --- the command line --------------------------------------------------------
run_case -x -s 0 f
run_case --nope
run_case -s
run_case -r
run_case -s 0 -- -f
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version

chmod -R u+rw "$DIFF_TMP" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
