#!/usr/bin/env bash
# Differential test: our `hostname` against net-tools `hostname`.
#
# ## Why this harness refuses to run as root, before anything else
#
# `hostname` is the only subject in this directory that **writes to the
# machine**. `hostname foo` sets the system's name, and there is no dry-run
# flag. As an ordinary user every such call is refused -- "you must be root to
# change the host name" -- which is a perfectly good deterministic case to
# compare, and is why the operand cases below are safe to have here at all.
#
# Run the same file as root and those cases stop being comparisons and become
# edits, twice each, to the host the other two lanes are also working on. So
# this harness checks `id -u` and refuses. That is not a general rule for the
# family; it is specific to the one subject whose failure mode is "it worked".
#
# ## What it is compared against
#
# net-tools `hostname` 3.23, from Debian's `hostname` package -- not GNU
# coreutils, which has no `hostname`. `DIFF_GNU_SOURCE` therefore cannot supply
# it and the reference is `/usr/bin/hostname`. §726's caveat applies in the mild
# form: this is a small package Debian does not carry behavioural patches
# against, unlike its coreutils.
#
# ## Why the host-dependent cases are safe to compare
#
# Every case here prints something about *this* machine, so the obvious worry is
# that the two sides see different answers a second apart. Measured before the
# harness was written: each of ``hostname``, ``-s``, ``-d``, ``-f``, ``-i``,
# ``-I`` and ``-b`` returns identical output on two consecutive runs on this
# host. The one to watch is ``-I``, which lists every interface address
# including IPv6 privacy addresses, and those do rotate -- on a timescale of
# hours, not of the milliseconds between our two invocations, but if this
# harness ever reports a lone ``-I`` difference that is the first thing to
# suspect rather than a defect.
#
# ## Why `od -An -c`
#
# `-I` is a space-separated list with a trailing space, and the difference
# between "one trailing space" and "none" is exactly the kind of thing a
# comparison that trimmed would call equal. The outputs are one short line each.
set -u

DIFF_PROG='hostname'
# Every invocation below is bounded with it, on both sides. `-i` and `-I` do
# name resolution, which is the one thing here that can block indefinitely --
# on a host whose resolver is pointed at something unreachable, an unbounded
# harness would hang rather than report.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# The refusal described in the header. After sourcing, so that the message goes
# through the same stderr the rest of the harness uses, and before any case has
# run.
if [ "$(id -u)" = "0" ]; then
  echo "hostname-diff: refusing to run as root." >&2
  echo "  Several cases below pass an operand, which as an ordinary user is" >&2
  echo "  refused with 'you must be root to change the host name' and is a" >&2
  echo "  comparison. As root the same cases would SET this machine's" >&2
  echo "  hostname, twice each, on a host two other lanes are using." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

printf 'namefromfile\n'   > name.txt
printf '\n'               > empty.txt
printf ''                 > nothing.txt
printf '  spaced  \n'     > spaced.txt
printf 'first\nsecond\n'  > two.txt
printf '# a comment\nafter\n' > comment.txt

run_side() {
  local side=$1; shift
  diff_run timeout -k 2 15 env LC_ALL=C.UTF-8 PATH="$bindir/$side" hostname "$@"
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
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

run_case() { compare "$@"; report "hostname $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS hostname %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail hostname %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the plain query ----------------------------------------------------------
run_case

# --- each query flag, short and long ------------------------------------------
run_case -s
run_case --short
run_case -d
run_case --domain
run_case -f
run_case --fqdn
run_case --long
run_case -i
run_case --ip-address
run_case -I
run_case --all-ip-addresses
run_case -A
run_case --all-fqdns
run_case -a
run_case --alias
run_case -y
run_case --yp
run_case --nis
run_case -b
run_case --boot

# --- long-option abbreviation --------------------------------------------------
run_case --sh
run_case --dom
run_case --fq
run_case --ip
run_case --all
run_case --a
run_case --s
run_case --f
run_case --l

# --- combinations, which net-tools mostly refuses -------------------------------
run_case -s -d
run_case -d -s
run_case -f -s
run_case -s -f
run_case -i -I
run_case -sd
run_case -fs

# --- reading a name from a file, which does NOT set it without privilege --------
run_case -F name.txt
run_case --file name.txt
run_case -F empty.txt
run_case -F nothing.txt
run_case -F spaced.txt
run_case -F two.txt
run_case -F comment.txt
run_case -F /nosuch/file
run_case -F
run_case -F /etc

# --- an operand, which is a SET and is refused: see the header -------------------
run_case newname
run_case -s newname
run_case some.fqdn.example
run_case ''
run_case -- -s

# --- refusals --------------------------------------------------------------------
run_case -Q
run_case --nosuchoption
run_case -sQ
run_case --
run_case -

# --- the two whose text is ours ----------------------------------------------------
xfail_case "our help text, not net-tools'" -h
xfail_case "our help text, not net-tools'" --help
xfail_case "our version string, not net-tools'" -V
xfail_case "our version string, not net-tools'" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
