#!/usr/bin/env bash
# id-diff.sh — compare our `id` and `groups` against GNU's, inside WSL.
#
# ## Why the two share a harness
#
# Upstream compiles `src/group-list.c` into both programs, and here both print
# through `coreutils::grouplist`. What one gets wrong about a group list the
# other gets wrong too, so each case that asks one program about a user is
# worth asking the other about the same user.
#
# ## What this is checking
#
#   * **the current process** -- `id`, each single-field form, `-n`, `-r`,
#     `-z`, `-G`; `groups` with no operand. Real supplementary groups, from
#     WSL's own account.
#   * **named accounts** -- by name, by number (`id` takes one, `groups` does
#     not: it is `getpwnam` only), with `+`, several at once, and an unknown one
#     among known ones (reported, the rest still answered, status 1).
#   * **the command line** -- the combinations `id` refuses and in what order,
#     unknown options, `--`.
#   * **write errors** -- `>/dev/full`.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='id'
DIFF_BINS='id groups'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
me=$(id -un)
TO_FULL=

compare() {
  local prog=$1; shift
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >/dev/full 2>"$err"
    else
      ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
    : >"$out"
  done
  TO_FULL=
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

run_case() {
  local label="$*${TO_FULL:+  [>/dev/full]}"
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

# --- the current process ------------------------------------------------------
run_case id
run_case id -u
run_case id -un
run_case id -ur
run_case id -g
run_case id -gn
run_case id -G
run_case id -Gn
run_case id -Gz
run_case id -Gnz
run_case id -a
run_case groups

# --- named accounts -----------------------------------------------------------
run_case id root
run_case id "$me"
run_case id -Gn "$me"
run_case id -G root
run_case id 0
run_case id +0
run_case id nosuchuser
run_case id root nosuchuser "$me"
run_case id -un root "$me"
run_case groups root
run_case groups "$me"
run_case groups root "$me"
run_case groups nosuchuser
run_case groups nosuchuser root
run_case groups 0
run_case groups +0
run_case groups ''

# --- the command line ---------------------------------------------------------
run_case id -r
run_case id -n
run_case id -z
run_case id -ug
run_case id -uG
run_case id -x
run_case id --nope
run_case id -- root
run_case groups -x
run_case groups --nope
run_case groups -- root
run_case groups --help=1
xfail_case 'our --help omits the GNU ancillary block' id --help
xfail_case 'our --version names SlateOS' id --version
xfail_case 'our --help omits the GNU ancillary block' groups --help
xfail_case 'our --version names SlateOS' groups --version
xfail_case 'our --help omits the GNU ancillary block' groups root --help

# --- write errors -------------------------------------------------------------
TO_FULL=1; run_case id
TO_FULL=1; run_case groups
TO_FULL=1; run_case groups root

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
