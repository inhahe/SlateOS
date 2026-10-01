#!/usr/bin/env bash
# sync-diff.sh — compare our `sync` against the real GNU one, inside WSL.
#
# ## What this is checking
#
# What `sync` flushes is not observable from here, so this compares what is:
# which operands it reports, in which words, and the status. The operands are
# chosen for the paths upstream takes before the sync itself:
#
#   * **the mode table** -- no operand (`sync()`, with or without `-f`), files,
#     `-d` files, `-f` files; and the two refusals, `--data` alone and both
#     flags together.
#   * **opening** -- a missing file; a write-only file (opened for writing
#     after the read fails, and synced); a file neither readable nor writable
#     (reported with the READ attempt's error); a directory; a FIFO (opened
#     non-blocking, so it cannot hang, and then refused by `fsync`);
#     `/dev/null`.
#   * **carrying on** -- a failure does not stop the files after it.
#   * **the command line** -- an unknown option, a value on a flag, `--`.
#
# Runs as an unprivileged user, which is what makes the permission cases
# mean anything; as root they would all open.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='sync'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

make_fixture() {
  local d=$1
  mkdir -p "$d"
  ( cd "$d" &&
    printf 'data\n' > f &&
    printf 'more\n' > g &&
    mkdir dir &&
    printf 'w\n' > wo && chmod 200 wo &&
    printf 'n\n' > none && chmod 000 none &&
    mkfifo fifo &&
    printf 'h\n' > -f )
}

compare() {
  local side dir out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    dir=$DIFF_TMP/case-$side
    chmod -R u+rw "$dir" 2>/dev/null; rm -rf "$dir"
    make_fixture "$dir"
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" sync "$@" ) >"$out" 2>"$err"
    rc=$?
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s}\n  gnu  (rc=%s): err{%s}' \
    "$o_rc" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

run_case() {
  local label="sync $*"
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
  local label="sync $*"
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

# --- the mode table ----------------------------------------------------------
run_case
run_case -f
run_case --file-system
run_case -d
run_case --data
run_case -d -f
run_case -df f
run_case --data --file-system f
run_case f
run_case f g
run_case -d f
run_case -f f g
run_case f -d

# --- opening -----------------------------------------------------------------
run_case missing
run_case -d missing
run_case -f missing
run_case wo
run_case -d wo
run_case none
run_case -f none
run_case dir
run_case -d dir
run_case -f dir
run_case fifo
run_case -d fifo
run_case -f fifo
run_case /dev/null
run_case ''
run_case f/

# --- carrying on -------------------------------------------------------------
run_case missing f none g
run_case -d none missing f

# --- the command line --------------------------------------------------------
run_case -x
run_case --nope
run_case --data=1
run_case -- -f
run_case -f -- -f
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --help omits the GNU ancillary block' f --help

chmod -R u+rw "$DIFF_TMP" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
