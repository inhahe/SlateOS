#!/usr/bin/env bash
# Differential test: our `ps` against procps-ng `ps`.
#
# ## Why this one was left until last
#
# `free` reports the machine and `uptime` reports the clock; both pin by
# bind-mounting one file. `ps` reports EVERY PROCESS, so there is no single
# file to pin and two runs a second apart legitimately differ. That is why
# `known-issues.md` carried this pair for weeks with "no harness -- write one"
# and nobody wrote one.
#
# ## What pins it
#
#     setsid unshare -pf --mount-proc -Ur sh -c '… exec ps "$@"' < /dev/null
#
# Three things are doing work there and two of them look like shell hygiene:
#
#   * `-pf --mount-proc` is the pin. A PID namespace with its own `/proc`
#     restarts PIDs at 1, so the PID and PPID columns are a fixture rather
#     than whatever the host happened to be running.
#   * `setsid` pins the TTY column. Without it that column reads `pts/2`
#     interactively and something else from a hook, so the harness would pass
#     by hand and fail in the gate for a reason that is not about `ps`.
#   * `setsid` ALSO silences WSL's "your 131072x1 screen size is bogus"
#     warning, which goes to STDERR. Without it every stderr comparison below
#     carries that line, and it would read as a difference in `ps`.
#
# `exec ps` rather than `ps` matters too: it replaces the shell, so PID 1 IS
# the subject and the table has exactly one row. Without the `exec` the shell
# survives as PID 1 and its own command line -- which contains this script's
# text -- appears in the CMD column.
#
# ## The bindir arrives by ENVIRONMENT, not by argument
#
# `$bindir/$side` must not reach the inner shell as an argument. `ps -ef`
# prints the command line of every process it can see, PID 1 included, so an
# argument naming `…/ours` or `…/gnu` would appear in the output and make the
# two sides differ for a reason that is a property of the harness. It is
# passed in `PS_DIFF_BIN` instead, and the environment is not printed.
#
# ## The one field that still moves
#
# `STIME` is the wall clock at minute granularity. Two runs in the same minute
# agree and two straddling a boundary differ by one, so it is compared with a
# tolerance rather than masked -- `uptime-diff.sh` carries the same argument at
# second granularity. A masked column tests nothing, and `\d\d:\d\d` is matched
# just as well by the wrong timezone as by the right answer.
set -u

DIFF_PROG='ps'
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; masked=0; broken=0

# Can we pin it?  Probed once, and the answer decides whether the cases below
# are evidence or are masked.  The probe demands the namespace AND that PID 1
# really is renumbered, because `unshare` succeeding while `--mount-proc`
# quietly did nothing would leave the host's table visible.
# The probe runs the command DIRECTLY rather than in a `$(…)`, which matters
# for a reason that has nothing to do with this harness reading correctly.
# `scripts/shell-callables.py` refuses a literal command substitution whose
# callee it cannot resolve, because a missing callee makes `$(…)` exit 127 and
# evaluate to the empty string -- and an empty string compared against "1" is
# a perfectly ordinary-looking `ns=none`. The harness would then mask every
# case and report it, which is honest, but it would be masking for a reason
# the operator could not see. `free-diff.sh` and `df-diff.sh` both test the
# command directly for the same reason. See known-issues ->
# A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED.
ns=none
if command -v unshare >/dev/null 2>&1 && command -v setsid >/dev/null 2>&1 \
   && setsid unshare -pf --mount-proc -Ur sh -c \
        'exec ps -o pid= -p 1' </dev/null 2>/dev/null | grep -q '^ *1 *$'; then
  ns=yes
fi

run_side() {
  local side=$1; shift
  PS_DIFF_BIN="$bindir/$side"
  export PS_DIFF_BIN
  # The script text below is IDENTICAL for both sides -- that is the point of
  # taking the bindir from the environment. It ends up in no process's command
  # line because `exec` replaces the shell before `ps` looks.
  diff_run timeout -k 2 20 setsid unshare -pf --mount-proc -Ur sh -c '
     PATH=$PS_DIFF_BIN:$PATH; export PATH
     LC_ALL=C.UTF-8; export LC_ALL
     TZ=UTC; export TZ
     exec ps "$@"' _ "$@" < /dev/null
}

# Minutes since midnight of an `HH:MM` STIME field, one per line.
stimes() {
  printf '%s' "$1" | sed -n 's/.*[^0-9]\([0-9][0-9]\):\([0-9][0-9]\)[^0-9].*/\1 \2/p' \
  | while read -r h m; do printf '%s\n' "$(( 10#$h * 60 + 10#$m ))"; done
}

# The output with every HH:MM replaced, so the rest compares byte for byte.
without_stime() {
  printf '%s' "$1" | sed 's/[0-9][0-9]:[0-9][0-9]\([^:0-9]\)/HH:MM\1/g'
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$(run_side ours "$@" 2>/dev/null); o_rc=$?
  o_err=$(run_side ours "$@" 2>&1 >/dev/null)
  g_out=$(run_side gnu  "$@" 2>/dev/null); g_rc=$?
  g_err=$(run_side gnu  "$@" 2>&1 >/dev/null)

  # NEITHER SIDE RUNNING IS NOT AGREEMENT. free-diff.sh reported 47 passes
  # when a PATH bug killed both invocations with rc=127, byte-identically.
  # 127 is command-not-found; 125 is the namespace refusing above.
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 125 ] || [ "$g_rc" = 125 ]; then
    AGREED=broken
    REPORT="  ours rc=$o_rc  gnu rc=$g_rc"
    return 0
  fi

  local o_body g_body skew=0 o_list g_list
  o_body=$(without_stime "$o_out"); g_body=$(without_stime "$g_out")

  # Compare the STIME fields pairwise. A differing COUNT of them is a real
  # difference in the table, not a skew, and must not be tolerated away.
  o_list=$(stimes "$o_out"); g_list=$(stimes "$g_out")
  if [ "$(printf '%s' "$o_list" | wc -l)" != "$(printf '%s' "$g_list" | wc -l)" ]; then
    skew=99999
  else
    local a b d
    while IFS= read -r a && IFS= read -r b <&3; do
      [ -z "$a" ] && continue
      d=$(( a - b )); [ "$d" -lt 0 ] && d=$(( -d ))
      # Midnight: 23:59 against 00:00 is one minute, not 1439.
      [ "$d" -gt 720 ] && d=$(( 1440 - d ))
      [ "$d" -gt "$skew" ] && skew=$d
    done <<EOF2 3<<EOF3
$o_list
EOF2
$g_list
EOF3
  fi

  if [ "$o_body" = "$g_body" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_err" = "$g_err" ] \
     && [ "$skew" -le 1 ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}\n  STIME skew: %s min' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$skew")
}

report() {
  local label="$1"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached `ps` on one or both sides\n%s\n' "$label" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() {
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  compare "$@"
  report "ps $*"
}

xfail_case() {
  local why=$1; shift
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  compare "$@"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN ps %s -- never reached `ps`\n' "$*"
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS ps %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail ps %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default and the two forms our coreutils bin claims -------------------
run_case
run_case -e
run_case -A
run_case -f
run_case -ef
run_case -e -f
run_case -fe
run_case -Ae
run_case -Af

# --- forms only the standalone claims ----------------------------------------
# These are the reason the pair is worth measuring rather than ranking: the
# standalone advertises -l, -o, -p, -t and -u, which coreutils' does not, and
# an option that is ADVERTISED is not the same as one that AGREES. `free`'s
# standalone advertised more and scored 0 of 48.
# All of these are real procps options that this build does not implement, so
# it refuses them and procps does not. That is the correct behaviour for an
# option we do not have -- §1006 -- and it is a divergence, not a bug, until
# the option exists. Declared with the reason naming WHICH option, so that
# implementing one turns its case into an XPASS rather than leaving a stale
# blanket excuse covering nine cases.
# `-l` is a fixed column set, measured so the eventual implementation has
# something to match rather than a name to guess from:
#
#   F S   UID     PID    PPID  C PRI  NI ADDR SZ WCHAN  TTY          TIME CMD
#   4 R     0       1       0  0  80   0 -  2093 -      ?        00:00:00 ps
#
# The UID is NUMERIC here -- `0`, where `-f` prints `root` for the same
# process in the same listing. Two format options, two renderings of one
# field, and the obvious shared helper would get one of them wrong.
xfail_case "-l (long format) is not implemented here" -l
xfail_case "-l (long format) is not implemented here" -el
xfail_case "-l (long format) is not implemented here" -efl
# `-u` is SELECTION BY USER, not a format, and the reason here said
# otherwise until it was measured. SysV `ps -u root` prints the DEFAULT
# columns for that user's processes; the user-oriented format is BSD `u`
# with no dash, which is a different option that happens to share a
# letter. `userspace/ps` documents "-u [user] User-oriented format" and
# so conflates them -- which is worth knowing before porting it in, since
# porting the standalone's `-u` would import the conflation.
#
# `ps -u` with no list exits 1.
# Bare `-u` with no list is NOT the error it looks like. procps prints the
# BSD user-oriented header -- `USER PID %CPU %MEM VSZ RSS TTY STAT START
# TIME COMMAND` -- on STDOUT with an empty stderr and exits 1. So it falls
# back to the `ps u` format with nothing selected, and matching it means
# implementing that format, not fixing this option. We print an error and
# the usage instead.
#
# Declared after measuring, not from the exit status: both sides exit 1 and
# a harness comparing only the status would have called this agreement.
xfail_case "bare -u needs the BSD user-oriented format, which is not implemented" -u
run_case -u root
run_case -u 0
run_case -u root,daemon
run_case -u nosuchuser
run_case -u 99999
run_case -p 1
xfail_case "-o (output column selection) is not implemented here" -o pid
xfail_case "-o (output column selection) is not implemented here" -o pid,comm
xfail_case "-o (output column selection) is not implemented here" -o comm=
run_case --no-header
run_case -e --no-header

# --- refusals ----------------------------------------------------------------
run_case --nosuchoption
run_case -Q
# `-o` still refuses on both sides for DIFFERENT reasons: we reject the option,
# procps accepts it and rejects the column name. Same exit status, different
# complaint, and it stays declared until `-o` exists.
#
# `-p` used to be in this group and has left it. That is what these per-option
# reasons are for: implementing `-p` turned its three cases into real
# comparisons instead of leaving a collective excuse covering an option that
# had since been written.
xfail_case "we reject -o itself; procps rejects the column name" -o nosuchcolumn
run_case -p notanumber
run_case -p 999999

# `-X` and `-h` are NOT unknown options, and putting them in the list above was
# my error rather than a finding. Both are real procps options this build does
# not implement: `-X` is the register format and prints its own header before
# exiting 1, and `-h` suppresses the header and exits 1 silently with no output
# at all on either stream. We refuse both, which is right for an option we do
# not have, and differs from a reference that has it.
#
# Measured before being declared, after `-ps` in uptime-diff.sh and `--help` in
# free-diff.sh were both declared divergences that were never true. An expected
# failure is an assertion, and it is the only kind its own case passing never
# tests.
xfail_case "procps implements -X (register format); this build does not" -X
xfail_case "procps implements -h (suppress header); this build does not" -h

# --- help and version --------------------------------------------------------
# NOT declared divergences without measuring. free-diff.sh shipped a `--help`
# xfail whose reason had never been true, and uptime-diff.sh declared `-ps` a
# refusal when it is not one. A declared divergence is an assertion like any
# other and is the only kind never tested by its own case passing.
run_case --help

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
if [ "$masked" -gt 0 ]; then
  printf ', %d MASKED' "$masked"
fi
if [ "$broken" -gt 0 ]; then
  printf ', %d BROKEN (never reached the subject)' "$broken"
fi
printf '\n'
if [ "$masked" -gt 0 ]; then
  cat <<'MASKEDNOTE'

Every case was masked: this host could not give the harness a PID namespace
with its own /proc, so the process table could not be pinned and both sides
would have been reading a machine that moves between them.  That is not a
comparison, so nothing was compared and nothing is claimed.  `unshare -pf
--mount-proc` and `setsid` are what is missing; `df-diff.sh` documents the
same class of requirement.
MASKEDNOTE
fi
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$masked" = 0 ] && [ "$broken" = 0 ]
