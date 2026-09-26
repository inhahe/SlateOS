#!/usr/bin/env bash
# tz-diff.sh — how `TZ` is read, ours against glibc's, through `date`.
#
# ## What this is checking
#
# Every program in the tree that prints a time reads `TZ` through
# `localtime::Zone`, whose `tzset` module is a port of glibc 2.39's
# `time/tzset.c` and `time/tzfile.c`. This drives `date` -- ours, and GNU's
# built from 9.4 against WSL's glibc -- over `TZ` values chosen for each of
# glibc's rules, at instants either side of the transitions those rules decide:
#
# | section | what it decides |
# |---|---|
# | 1. `date -f` of instants | unset is `/etc/localtime`; empty is `Universal`; `:` is dropped; a file is tried before a rule; a rule keeps what parses of it (`Foo/Bar` is UTC named `Foo`, `EST5x` has an unnamed DST half); offsets are `sscanf`'s; a DST name with no dates takes `posixrules`' history, anchored by glibc's process-wide `rule_dstoff`; every year up to 1970 changes on 1970's dates; the year is the UTC one |
# | 2. `date -d` of local times, one process each | glibc's `mktime` re-reads a `posixrules` zone, which moves its fall transitions |
# | 3. the same local times as one `date -f` | the order of those reads across lines |
# | 4. `TZ="…"` date strings | gnulib switches `TZ` to the string's zone and back, which reads both |
#
# The instants are listed with `@` because a `date -f` line of `@N` converts
# without `mktime`: section 1 sees only `localtime_r`, and what `mktime` adds
# is sections 2-4's.
#
# ## Cases that differ on purpose
#
# One, and it is a decision, not a defect: `TZ=../zoneinfo/UTC`. A name with a
# `..` component is never read as a file here, which glibc does only in a
# setuid program (see `localtime`'s `zoneinfo_path`); the value falls through
# to the POSIX rule, a nameless UTC, where glibc reads the file and says
# `UTC`. It is run as an xfail so that the difference stays visible and stays
# the only one.
set -u

DIFF_PROG='date'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
work=$DIFF_TMP/work
mkdir -p "$work"

FORMAT='+%s %F %T %Z %z'

# `run_side SIDE TZ ARGS...`: `date ARGS` under TZ (`<unset>`: none), with the
# exit status appended so that a refusal is compared too.
run_side() {
  local side=$1 tz=$2; shift 2
  if [ "$tz" = '<unset>' ]; then
    { timeout -k 2 30 env -u TZ LC_ALL=C.UTF-8 PATH="$bindir/$side" date "$@"; echo "rc=$?"; } 2>&1
  else
    { timeout -k 2 30 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/$side" date "$@"; echo "rc=$?"; } 2>&1
  fi
}

# `check LABEL TZ ARGS...`: one case.
check() {
  local label=$1 tz=$2; shift 2
  local o g
  o=$(run_side ours "$tz" "$@")
  g=$(run_side gnu "$tz" "$@")
  if [ "$o" = "$g" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    diff <(printf '%s\n' "$o") <(printf '%s\n' "$g") | head -12
  fi
  return 0
}

# `xfail LABEL TZ ARGS...`: a difference the header above explains.
xfail() {
  local label=$1 tz=$2; shift 2
  if [ "$(run_side ours "$tz" "$@")" = "$(run_side gnu "$tz" "$@")" ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (the header says this differs -- update it)\n' "$label"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s\n' "$label"
  fi
  return 0
}

# Both sides must actually start; see `strftime-diff.sh` for what a harness
# whose binaries are both missing reports.
for side in ours gnu; do
  if ! env PATH="$bindir/$side" date +%s >/dev/null 2>&1; then
    echo "tz-diff: no working \`date\` in $bindir/$side" >&2
    exit 1
  fi
done

TZS=(
  '<unset>' '' ':' 'Universal' 'UTC' 'UTC0' 'GMT' 'Foo' 'Foo/Bar' '12' 'Ab5'
  '<+05>-5' '<-00>0' '<AB>5' '<ABC5'
  'EST5EDT' ':EST5EDT' 'CST6CDT' 'EST5' 'EST+5' 'EST-5' 'EST25' 'EST99:99:99'
  'EST+ 5' 'EST+-5' 'EST5:30' 'EST5x' 'EST5:' 'EST5EDT 4'
  'EST5EDT4' 'AAA3BBB' 'AAA3BBB,' 'CET-1CEST' 'AAA3BBB,M4.1.0,M10.1.0'
  'EST5EDT,M3.2.0,M11.1.0' 'CET-1CEST,M3.5.0,M10.5.0/3'
  'AEST-10AEDT,M10.1.0,M4.1.0/3' 'EST5,M3.2.0,M11.1.0'
  'EST5EDT,J60,J300' 'EST5EDT,59,299' 'AAA0BBB,J60/0,J61/0'
  'EST5EDT,M3.2.0/-1,M11.1.0/26' 'EST5EDT,M3.5.0,M10.5.0/167'
  'AAA3BBB,J' 'AAA3BBB,M3' 'AAA3BBB,M3.2.0' 'EST5EDT,M3.2.0x,M11.1.0'
  'EST5EDT,Q' 'AAA-12BBB,J1/0,J1/10'
  'America/New_York' ':America/New_York' '/usr/share/zoneinfo/Asia/Kolkata'
  'Australia/Lord_Howe' 'Europe/Moscow' 'Etc/GMT+5' 'nonexistent/zone'
  'posixrules' 'America' 'America/'
)

INSTANTS=(
  0 -1 1000000000 1609459199 1609419600
  # New York's 2020, and where posixrules re-anchoring moves it under AAA3BBB.
  1583643600 1583650799 1583650800 1583657999 1583658000
  1604196000 1604203199 1604203200 1604210399 1604210400 1604214000
  # 2021's spring, and summers either side of 1970.
  1615705199 1615705200 1623758400 -14182940 -1000000000
  # Years 21, 0, 1969, 2040 and 5138.
  -61490188800 -62150000000 -31536000 2224800000 100000000000
)

# --- 1. instants, by `date -f` ------------------------------------------------------
instants=$work/instants
printf '@%s\n' "${INSTANTS[@]}" >"$instants"
for tz in "${TZS[@]}"; do
  check "TZ=$tz date -f instants" "$tz" -f "$instants" "$FORMAT"
done
xfail "TZ=../zoneinfo/UTC date -f instants" '../zoneinfo/UTC' -f "$instants" "$FORMAT"

# --- 2. local times, one process each ------------------------------------------------
# Each converts through `mktime`, whose `tzset ()` re-reads a `posixrules`
# zone: the first read in the process anchored it by 0, this one by the user's
# DST offset.
LOCALS=(
  '2020-11-01 00:30' '2020-11-01 01:30' '2020-11-01 02:30' '2020-11-01 03:30'
  '2020-03-08 01:30' '2020-03-08 02:30' '2020-03-08 05:30' '2020-07-01 12:00'
  '2020-12-31 23:59' '0000-06-01 00:00' '0021-06-15 12:00' '1969-07-01 12:00'
)
LOCAL_TZS=(
  '<unset>' 'AAA3BBB' 'CET-1CEST' 'EST5EDT4' 'EST5EDT' 'EST5x' 'Foo/Bar'
  'CET-1CEST,M3.5.0,M10.5.0/3' 'AEST-10AEDT,M10.1.0,M4.1.0/3' 'Etc/GMT+5'
)
for tz in "${LOCAL_TZS[@]}"; do
  for s in "${LOCALS[@]}"; do
    check "TZ=$tz date -d '$s'" "$tz" -d "$s" "$FORMAT"
  done
done

# --- 3. the same, as one process ----------------------------------------------------
locals=$work/locals
printf '%s\n' "${LOCALS[@]}" >"$locals"
for tz in "${LOCAL_TZS[@]}"; do
  check "TZ=$tz date -f locals" "$tz" -f "$locals" "$FORMAT"
done

# --- 4. `TZ="…"` in the string --------------------------------------------------------
# Each switches `TZ` in and back out, reading the string's zone and then the
# process's own again -- in that order, which decides how each `posixrules`
# zone is anchored.
PREFIXED=(
  'TZ="CCC4DDD" 2020-11-01 00:30' 'TZ="AAA3BBB" 2020-11-01 01:30'
  'TZ="Etc/GMT+5" 2020-11-01 01:30' 'TZ="EST5x" 0000-06-01 00:00'
  'TZ="America/New_York" 2020-11-01 01:30' 'TZ="" 2020-07-01 12:00'
  'TZ="Foo/Bar" 2020-07-01 12:00' 'TZ="AAA3BBB" 2020-11-01 01:30 +1 day'
)
for tz in '<unset>' 'AAA3BBB' 'UTC0' 'Etc/GMT+5' 'CCC4DDD'; do
  for s in "${PREFIXED[@]}"; do
    check "TZ=$tz date -d '$s'" "$tz" -d "$s" "$FORMAT"
  done
  prefixed=$work/prefixed
  printf '%s\n' "${PREFIXED[@]}" "${LOCALS[@]}" >"$prefixed"
  check "TZ=$tz date -f prefixed+locals" "$tz" -f "$prefixed" "$FORMAT"
done

# The wording is the family's: `all-diff.sh` decides green by matching
# " 0 differed" in this line.
printf '\ntz: %d passed, %d differed (%d xfail, %d xpass)\n' "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
