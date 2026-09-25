#!/usr/bin/env bash
# printenv-diff.sh — compare our `printenv` against the real GNU one, inside WSL.
#
# ## What this is checking
#
# `printenv` is a lookup with three answers -- found, not found, and "you asked
# wrongly" -- and each has its own exit status: 0, 1 and 2. Most of what can go
# wrong is in which one a given command line earns, so the cases are grouped by
# that:
#
#   * **the listing** -- no operands prints every `NAME=VALUE`, newline- or
#     NUL-terminated.
#   * **lookups** -- found, missing, a mix (prints what it found, exits 1), the
#     empty name and a name holding `=` (never found), and a value that is not
#     UTF-8 (printed as the bytes it is).
#   * **option parsing** -- `+` stops at the first operand, so `printenv A -0`
#     looks up `-0`; `-i` and `-u X` are accepted by getopt and refused with
#     only the `Try` line; `-u` alone is getopt's own complaint.
#   * **the write errors** -- `>&-` and `>/dev/full`, status 2.
#
# Every case runs under `env -i` with a known environment. Both sides find
# `printenv` on the SAME `PATH` -- one scratch directory whose `printenv` link
# is pointed at each side's binary in turn -- because the listing prints `PATH`
# too, so per-side directories would make every listing differ for a reason
# that is the harness's own; and because running a binary by absolute path
# makes GNU name itself by that whole path in every diagnostic.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `USAGE_BUILTIN_WARNING`, and
# `--version` names SlateOS. Both are `xfail`.
#
# Run `OURS=/usr/bin/printenv ./scripts/printenv-diff.sh` to confirm the
# harness still discriminates: it should report every xfail as XPASS and
# nothing else.
set -u

DIFF_PROG='printenv'
# Not the installed binary: see `diff-wsl.sh`'s "Why a built reference".
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# The one directory both sides are found in; see the header.
runbin=$DIFF_TMP/run
mkdir -p "$runbin"

# --- knobs, reset before every case ------------------------------------------
# `ENVV` is the whole environment, as words handed to `env -i`. `TO_FULL` and
# `CLOSED` are the two ways to reach a write error.
ENVV=; TO_FULL=; CLOSED=; ERR_CLOSED=
reset_knobs() { ENVV='A=1 B=x=y EMPTY='; TO_FULL=; CLOSED=; ERR_CLOSED=; }
reset_knobs

render() {
  local f=$1 sz
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  od -An -c <"$f"
}

compare() {
  local o_bin g_bin o_err g_err o_rc g_rc side out err rc
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_bin; err=$o_err
    else out=$g_bin; err=$g_err; fi
    ln -sfn "$bindir/$side/printenv" "$runbin/printenv"
    # `$ENVV` is deliberately unquoted: it is a word list.
    if [ -n "$CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env -i $ENVV PATH="$runbin" printenv "$@" ) >&- 2>"$err"
    elif [ -n "$TO_FULL" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env -i $ENVV PATH="$runbin" printenv "$@" ) >/dev/full 2>"$err"
    elif [ -n "$ERR_CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env -i $ENVV PATH="$runbin" printenv "$@" ) >"$out" 2>&-
    else
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env -i $ENVV PATH="$runbin" printenv "$@" ) >"$out" 2>"$err"
    fi
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  local o_out g_out o_msg g_msg
  o_out=$(render "$o_bin"); g_out=$(render "$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

label_of() {
  printf 'printenv %s  [env: %s]%s%s%s' "$*" "$ENVV" \
    "${TO_FULL:+  [>/dev/full]}" "${CLOSED:+  [>&-]}" "${ERR_CLOSED:+  [2>&-]}"
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  reset_knobs
  return 0
}

xfail_case() {
  local why="$1"; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  reset_knobs
  return 0
}

# --- the listing -------------------------------------------------------------
run_case
run_case -0
run_case --null
run_case --nu
ENVV=; run_case
ENVV=; run_case -0

# --- lookups -----------------------------------------------------------------
run_case A
run_case B
run_case EMPTY
run_case NOPE
run_case A NOPE B
run_case NOPE NOPE
run_case -0 A B
run_case ''
run_case '' A
# A name holding `=` is never found -- even `B=x`, whose entry is `B=x=y`.
run_case B=x
run_case A=1
run_case =
# Exact, not a prefix, in either direction.
ENVV='AB=1'; run_case A
ENVV='A=1'; run_case AB
# A value that is not UTF-8 is printed as the bytes it is.
ENVV=$(printf 'V=a\377b'); run_case V
ENVV=$(printf 'V=a\377b'); run_case

# --- option parsing ----------------------------------------------------------
run_case A -0
run_case -- -0
run_case -- A
run_case -i
run_case -u X
run_case -uX
run_case -u
run_case -i --help
run_case -0 -i
run_case -x
run_case --nope
run_case --null=1
run_case ---null

# --- help and version --------------------------------------------------------
xfail_case 'our --help omits the GNU ancillary block and the builtin warning' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --help omits the GNU ancillary block and the builtin warning' --he
run_case --help=1

# --- write errors ------------------------------------------------------------
CLOSED=1; run_case
CLOSED=1; run_case A
CLOSED=1; run_case NOPE
TO_FULL=1; run_case
TO_FULL=1; run_case A NOPE
# The usage error never reaches a flush, so a closed stdout adds nothing.
CLOSED=1; run_case -x
# A diagnostic that could not be delivered replaces the status.
ERR_CLOSED=1; run_case -x
ERR_CLOSED=1; run_case -i
ERR_CLOSED=1; run_case NOPE

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
