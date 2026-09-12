#!/usr/bin/env bash
# Differential test: our `uptime` against procps-ng `uptime`.
#
# ## Why this exists now and did not before
#
# `known-issues.md` said, under B-COREUTILS-UPTIME-SILENTLY-IGNORES-EVERY-
# ARGUMENT: "No harness was written for this pair, deliberately. `uptime`'s
# output *is* the current moment." That was true of the technique available
# when it was written, and stopped being true when `df-diff.sh` and then
# `free-diff.sh` established per-case `unshare -mUr` with the inputs
# bind-mounted. `/proc/uptime` and `/proc/loadavg` pin exactly as
# `/proc/meminfo` does.
#
# ## Three of the four fields pin. The fourth is the whole design problem.
#
#     08:59:34 up  1:01,  1 user,  load average: 0.01, 0.02, 0.03
#     ^^^^^^^^    ^^^^^   ^^^^^^                 ^^^^^^^^^^^^^^^^
#     clock       /proc/  utmp                   /proc/loadavg
#                 uptime  (see below)
#
# The clock is not in any file, so the two sides read it at different instants
# and a case straddling a second boundary differs for a reason that is a
# property of neither program. `date-diff.sh` names the cost: "a gate that
# fails once an hour for a reason nobody can reproduce is a gate that gets
# switched off." `date` escaped it by making every case a function of its
# arguments; `uptime` cannot, because it has no argument that sets the time.
#
# This host has no `faketime`, so the clock is compared with a TOLERANCE
# rather than masked. Masking would have been the weaker choice twice over: a
# masked field tests nothing, and `\d\d:\d\d:\d\d` is matched just as well by a
# twelve-hour clock or the wrong zone as by the right answer. Comparing the two
# readings as NUMBERS catches all three -- a zone error moves them by hours, a
# twelve-hour clock by up to twelve, and a padding error fails to parse -- while
# tolerating only the sub-second skew that is genuinely unavoidable.
#
# ## The user count pins too, which took two experiments
#
# On this host `uptime` links `libsystemd` and asks logind. Emptying
# `/run/utmp` inside the namespace leaves the count at 1 while `who`, which
# does read utmp, drops to 0 -- from which the first pass concluded the field
# could never be pinned. Masking `/run/systemd` as well makes procps fall back
# to utmp and the count becomes a fixture like everything else.
#
# The first experiment answered a narrower question than the one being asked,
# and its answer looked general. Worth remembering before writing "cannot be
# tested" anywhere: that sentence is a claim about the experiment that was run.
#
# Fixture utmp files are built by concatenating the host's own `/run/utmp` N
# times, so the records are real ones and the count is exactly N.
set -u

DIFF_PROG='uptime'
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; masked=0; broken=0

# --- the pinned machine -------------------------------------------------------
fixdir=$DIFF_TMP/fix
mkdir -p "$fixdir/emptydir"
: > "$fixdir/utmp0"
# 16000 s = 4:26, chosen so hours, minutes and the space-padding of the hour
# are all exercised by the DEFAULT case rather than only by the sweep below.
printf '16000.00 100000.00\n' > "$fixdir/uptime"
printf '0.07 1.05 12.34 2/297 51893\n' > "$fixdir/loadavg"

# Real utmp records, N copies of them, so the count is N and the bytes are
# not invented.  N=0 is an empty file.
for n in 1 2 12; do
  : > "$fixdir/utmp$n"
  i=0
  while [ "$i" -lt "$n" ]; do
    cat /run/utmp >> "$fixdir/utmp$n" 2>/dev/null || break
    i=$((i + 1))
  done
done

# Can we pin it?  Probed once, and the answer decides whether the cases below
# are evidence or are masked.  The probe checks the SYSTEMD mask too, because
# without it the user count is logind's and three of the cases below would be
# comparing a number neither side was given.
ns=none
if command -v unshare >/dev/null 2>&1 \
   && unshare -mUr sh -c "
        mount --bind '$fixdir/uptime' /proc/uptime 2>/dev/null || exit 1
        mount --bind '$fixdir/emptydir' /run/systemd 2>/dev/null || exit 1
        grep -q '^16000' /proc/uptime" 2>/dev/null; then
  ns=yes
fi

# $1 = side, $2 = utmp fixture, rest = argv
run_side() {
  local side=$1 utmp=$2; shift 2
  diff_run timeout -k 2 20 unshare -mUr sh -c '
     mount --bind "$1" /proc/uptime   || exit 125
     mount --bind "$2" /proc/loadavg  || exit 125
     mount --bind "$3" /run/utmp      || exit 125
     # Masking logind is what makes the user count a fixture; without it
     # procps answers from sessions this harness does not control.
     mount --bind "$4" /run/systemd   || exit 125
     PATH=$5:$PATH; export PATH
     LC_ALL=C.UTF-8; export LC_ALL
     TZ=UTC; export TZ
     shift 5
     exec uptime "$@"' \
    _ "$fixdir/uptime" "$fixdir/loadavg" "$utmp" "$fixdir/emptydir" \
    "$bindir/$side" "$@"
}

# Seconds since midnight of a leading " HH:MM:SS", or empty if absent.
clock_secs() {
  printf '%s' "$1" | sed -n \
    's/^ \([0-9][0-9]\):\([0-9][0-9]\):\([0-9][0-9]\) .*/\1 \2 \3/p' \
  | { read -r h m s || exit 0
      printf '%s' "$(( 10#$h * 3600 + 10#$m * 60 + 10#$s ))"; }
}

# The line with its clock replaced, so the rest compares byte for byte.
without_clock() {
  printf '%s' "$1" | sed 's/^ [0-9][0-9]:[0-9][0-9]:[0-9][0-9] / <CLOCK> /'
}

compare() {
  local utmp=$1; shift
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$(run_side ours "$utmp" "$@" 2>/dev/null); o_rc=$?
  o_err=$(run_side ours "$utmp" "$@" 2>&1 >/dev/null)
  g_out=$(run_side gnu  "$utmp" "$@" 2>/dev/null); g_rc=$?
  g_err=$(run_side gnu  "$utmp" "$@" 2>&1 >/dev/null)

  # NEITHER SIDE RUNNING IS NOT AGREEMENT.  free-diff.sh learned this the
  # expensive way: a PATH bug killed both invocations with rc=127 and 47 cases
  # reported a clean sweep.  127 is command-not-found, 125 is a bind-mount
  # failing above; both mean the case never reached `uptime`.
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 125 ] || [ "$g_rc" = 125 ]; then
    AGREED=broken
    REPORT="  ours rc=$o_rc  gnu rc=$g_rc"
    return 0
  fi

  local o_body g_body o_clock g_clock skew=0
  o_body=$(without_clock "$o_out"); g_body=$(without_clock "$g_out")
  o_clock=$(clock_secs "$o_out");   g_clock=$(clock_secs "$g_out")

  # A clock on one side only is a real difference, not a skew to tolerate.
  if [ -n "$o_clock" ] && [ -n "$g_clock" ]; then
    skew=$(( o_clock - g_clock ))
    [ "$skew" -lt 0 ] && skew=$(( -skew ))
    # Midnight: 23:59:59 vs 00:00:01 is 2 seconds, not 86398.
    [ "$skew" -gt 43200 ] && skew=$(( 86400 - skew ))
  elif [ -n "$o_clock" ] || [ -n "$g_clock" ]; then
    skew=99999
  fi

  if [ "$o_body" = "$g_body" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_err" = "$g_err" ] \
     && [ "$skew" -le 2 ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}\n  clock skew: %ss' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$skew")
}

report() {
  local label="$1"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached `uptime` on one or both sides\n%s\n' "$label" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# A case with the default utmp fixture (one session).
run_case() {
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  compare "$fixdir/utmp1" "$@"
  report "uptime $*"
}

# A case pinning the session count.
run_users() {
  local n=$1; shift
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  compare "$fixdir/utmp$n" "$@"
  report "uptime $* [${n} session(s)]"
}

# A case run against a chosen /proc/uptime value.
run_uptime() {
  local secs=$1; shift
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  printf '%s.00 100000.00\n' "$secs" > "$fixdir/uptime"
  compare "$fixdir/utmp1" "$@"
  report "uptime $* [/proc/uptime = ${secs}s]"
  printf '16000.00 100000.00\n' > "$fixdir/uptime"
}

# An expected divergence at a chosen /proc/uptime value.
xfail_uptime() {
  local secs=$1 why=$2; shift 2
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  printf '%s.00 100000.00\n' "$secs" > "$fixdir/uptime"
  compare "$fixdir/utmp1" "$@"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN uptime %s [%ss] -- never reached `uptime`\n' "$*" "$secs"
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS uptime %s [%ss] -- expected to differ (%s) and did not\n' "$*" "$secs" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail uptime %s [%ss] (%s)\n' "$*" "$secs" "$why"
  fi
  printf '16000.00 100000.00\n' > "$fixdir/uptime"
  return 0
}

xfail_case() {
  local why=$1; shift
  if [ "$ns" != yes ]; then masked=$((masked + 1)); return 0; fi
  compare "$fixdir/utmp1" "$@"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN uptime %s -- never reached `uptime`\n' "$*"
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS uptime %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail uptime %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default line ---------------------------------------------------------
run_case

# --- the `up …` field, every boundary the format has --------------------------
# Under an hour procps prints `N min` and no clock; at an hour the hour is
# SPACE padded; the day prefix changes neither rule. All three were wrong here
# until 2026-09-12 and none is what a from-scratch implementation writes.
for s in 0 5 59 60 61 119 3599 3600 3601 3660 7199 7200 86399 86400 86460 \
         90000 172800 259200 604800; do
  run_uptime "$s"
done

# --- the user count, including the value that is singular ---------------------
# `,  0 user` -- SINGULAR at zero. `n == 1 ? "" : "s"` gets every other row
# right, and gets wrong the one a machine with nobody logged in prints.
# 12 is here because `%2d` and a two-space literal plus `%d` agree on every
# single-digit count and part company at ten.
for n in 0 1 2 12; do
  run_users "$n"
done

# --- the other two forms ------------------------------------------------------
run_case -p
run_case --pretty
run_case --pret
run_case -s
run_case --since
run_case --si

# `-p` BELOW the first unit boundary, where the two agree.
for s in 0 59; do
  run_uptime "$s" -p
done

# `-p` AT AND ABOVE a unit boundary, where procps-ng 4.0.4 is wrong and we are
# not. This is the one place in this harness where the reference loses.
#
# Every level of procps' decomposition rolls over only when the remainder
# EXCEEDS the unit, never when it equals it, so an exact boundary falls through
# to the unit below -- and the last level has nothing below it. Measured by
# sweeping `/proc/uptime`:
#
#       60 -> `up `                     <- EMPTY. One minute of uptime.
#     3599 -> `up 59 minutes`
#     3600 -> `up 60 minutes`
#     3660 -> `up 1 hour`               <- and the remaining minute vanishes
#    86400 -> `up 24 hours, 0 minutes`
#    90000 -> `up 1 day, 60 minutes`    <- the hour became minutes
#   604800 -> `up 7 days, 0 minutes`
#
# Reproducing that bug-for-bug was considered and rejected. §371 makes
# bug-for-bug reproduction the default because it is what lets a measurement be
# asserted -- but the default assumes the reference's answer is *an* answer.
# `up ` is not a wrong rendering of one minute, it is no rendering at all, and
# a program whose whole job is one line answering with an empty one is the case
# §1006 exists for. Our `pretty()` is transcribed from procps' SOURCE, whose
# modulo arithmetic is the intent this binary fails to implement.
#
# Recorded in known-issues.md; not filed upstream, since this tree does not
# carry procps patches.
for s in 60 3600 86400 172800 604800 1209600 31536000 34560000 315360000; do
  xfail_uptime "$s" "procps-ng 4.0.4 rolls over on > rather than >=" -p
done

# --- refusals, where a stub and a real implementation part company ------------
# The EXIT STATUS and the refusal itself agree; the text after it does not, and
# the difference is a house style rather than a defect on either side.
#
#   ours:   uptime: invalid option -- 'Z'
#           Try 'uptime --help' for more information.
#   procps: uptime: invalid option -- 'Z'
#           <blank>
#           <the entire --help text>
#
# GNU coreutils prints the `Try …` hint; procps prints its whole usage. This
# tree's `coreutils::getopt` implements the GNU style for all 86 bins, and
# `uptime` is the one whose upstream is procps. Changing the shared formatter
# to match this single program would make the other 85 wrong, so the divergence
# is declared here instead of chased -- the refusal and the status, which are
# what a script reads, do match.
for a in -Z --nosuchoption extra-operand --pretty=value; do
  xfail_case "procps prints its full usage after an error; ours prints GNU's hint" "$a"
done
xfail_case "procps prints its full usage after an error; ours prints GNU's hint" -p extra

# `-ps` is NOT an error and this harness is what told me. I put it in the
# refusal list above on the assumption that a clustered pair would be rejected;
# it is `-p` then `-s`, both valid, last one winning, and the two sides agree
# exactly. The XPASS was the harness refusing a divergence I had declared
# without measuring -- the second time in two days, after `free --help`, whose
# stated reason had also never been true.
#
# Declaring an expected failure is an assertion like any other, and it is the
# one kind that is never tested by the case passing.
run_case -ps
run_case -sp

# `-s` is an EARLY EXIT, not a flag, and these are the cases that prove it.
# procps prints the boot time and leaves before the rest of argv is examined,
# so an invalid option or an operand after `-s` is never reached and the exit
# status is 0. Two models fitted `-ps`/`-sp`; feeding `-s` something it should
# have rejected is what separated them.
run_case -s junk
run_case -sZ
run_case -sV
run_case -hV
# `-Vs` and `-Vh` agree on WHICH option wins -- both sides print the version --
# and then differ only in the version string, which is already declared below.
# They stay as cases because the thing being tested is the precedence, and a
# divergence in the payload must not be able to hide agreement in the order.
xfail_case "our version string, not procps-ng's" -Vs
xfail_case "our version string, not procps-ng's" -Vh

# --- help ---------------------------------------------------------------------
# NOT declared a divergence. free-diff.sh shipped a `--help` xfail whose stated
# reason had never been true -- our `free --help` was byte-identical to
# procps-ng's from the first run -- so this one is a real case until measured
# otherwise, rather than assumed to be ours because help text usually is.
run_case --help
run_case -h

# `--version` really does differ: ours says "uptime (SlateOS coreutils)"
# against "uptime from procps-ng 4.0.4".
xfail_case "our version string, not procps-ng's" --version
xfail_case "our version string, not procps-ng's" -V

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

Every case was masked: this host could not give the harness a private mount
namespace, so `/proc/uptime`, `/proc/loadavg` and `/run/utmp` could not be
pinned and both sides would have been reading a machine that moves between
them.  That is not a comparison, so nothing was compared and nothing is
claimed.  `unshare -mUr` is what is missing; `df-diff.sh` documents the same
requirement.
MASKEDNOTE
fi
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$masked" = 0 ] && [ "$broken" = 0 ]
