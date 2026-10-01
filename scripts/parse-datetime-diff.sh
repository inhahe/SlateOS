#!/usr/bin/env bash
# parse-datetime-diff.sh — GNU's date language, ours against coreutils 9.4's.
#
# ## What this is checking
#
# `coreutils::parse_datetime` is a port of gnulib's `parse-datetime.y`: the
# Bison tables GNU shipped, the actions, the lexer and `parse_datetime_body`,
# with glibc's `mktime` ported into `localtime` beside it. This harness drives
# the one corpus of date strings through every program that reads the
# language, because each exposes a different part of it:
#
# | through | exposes | deterministic? |
# |---|---|---|
# | `touch -r REF -d STRING` | the instant, to the nanosecond, relative to REF's times | yes: "now" is REF |
# | `date -d STRING +FORMAT` | the instant as the zone renders it, and the refusals | to the second; retried once |
# | `date --debug -d STRING` | every part the parser recognised, in the order the LALR tables reduce them, and every warning | to the second; retried once |
# | `date -f FILE` | the whole corpus as one process, so `mktime`'s process-wide offset guess carries from line to line as glibc's does | to the second; retried once |
#
# ## Why `touch -r` is the oracle for "now"
#
# Half the language is relative to the current time (`now`, `3 days ago`,
# `tuesday`, `12:00`), and two processes run a few milliseconds apart cannot be
# made to agree about what time it is. `touch -r REF -d STRING` parses STRING
# relative to REF's timestamps instead of the clock -- upstream's
# `date_relative (flex_date, newtime[i])` -- so the same strings become a pure
# function of their arguments. Both sides' results are read back with the one
# GNU `stat`, so only the parsing differs.
#
# The `date` rows still run the whole corpus, for what only `date` shows (the
# zone's rendering, `--debug`, `-f`), and fold the two ways a clock race shows
# up: nanoseconds are not printed, and a row that differs is run again once
# before it counts -- two consecutive straddles of a second boundary are what
# it would take to fool it.
#
# ## Zones
#
# UTC, three zoneinfo zones chosen for their transitions (New York's hour,
# Lord Howe's half-hour DST, Kolkata's half-hour offset with none), and a
# POSIX rule -- whose transitions, before 1970, are 1970's, as glibc computes
# them. How each kind of `TZ` value is *read* is `tz-diff.sh`'s question, not
# this parser's.
#
# ## Cases that differ on purpose
#
# None.
set -u

DIFF_PROG='parse-datetime'
DIFF_BINS='date touch'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='timeout stat'
# A family: there is no one program called `parse-datetime` to compare against;
# each of `DIFF_BINS` is reached through `$bindir/{ours,gnu}`.
DIFF_NO_REF=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; kbug=0; kfixed=0
work=$DIFF_TMP/work
mkdir -p "$work"

ZONES=(
  UTC0
  America/New_York
  Australia/Lord_Howe
  Asia/Kolkata
  'CET-1CEST,M3.5.0,M10.5.0/3'
)

# The instants REF files carry, for `touch -r`. A Tuesday in a northern
# summer, with every nanosecond digit set; the first 01:30 of New York's
# fall-back day; the last second before its spring-forward; and a southern
# summer Sunday.
REFS=(
  @1623758400.123456789
  @1636263000
  @1615705199
  @1609583400.5
)

# The corpus. Grouped by what each group exercises; every group has rows that
# must be refused as well as rows that must be accepted.
CORPUS=(
  # --- ISO 8601 and friends
  '2021-06-15' '2021-06-15 12:00' '2021-06-15 12:00:00' '2021-06-15T12:00:00'
  '2021-06-15T12:00:00Z' '2021-06-15 12:00:00 UTC' '2021-06-15 12:00:00 +0530'
  '2021-06-15 12:00:00 -05:00' '2021-06-15 12:00:00.5' '2021-06-15 12:00:00,25'
  '2021-06-15T12:00:00.123456789123+01:00' '2021-06-15 12:00:00+00:00'
  '2021-06-15t12:00' '2021-06-15 T 12:00' '2021-6-5' '21-06-15' '0021-06-15'
  # --- other date orders
  '06/15/2021' '06/15/21' '6/15' '2021/06/15' '15 Jun 2021' 'Jun 15 2021'
  'Jun 15, 2021' '15-Jun-2021' 'Jun-15-2021' 'June 15' '15 june' 'JUNE 15 2021'
  'Sept 15' 'sep. 15' 'Tue Jun 15 12:00:00 UTC 2021'
  'Tue, 15 Jun 2021 12:00:00 +0000' '15 Jun' '1/2/69' '1/2/68' '12/31/99'
  # --- bare numbers
  '20210615' '20210615 1200' '210615' '1200' '12' '0' '2400' '1260' '123456'
  '2021-06-15 2021' '20210615 +1 day'
  # --- the far and the odd
  '1970-01-01 00:00:00 UTC' '1969-12-31 23:59:59 UTC' '1900-01-01' '0001-01-01'
  '9999-12-31' '10000-01-01' '2147485547-12-31' '2147485548-01-01'
  '17 JUN -1992' '17 JUN +1992'
  # --- seconds since the epoch
  '@0' '@-1' '@1.5' '@-1.5' '@1623758400.123456789' '@ 5' '@+5' '@1e3' '@'
  '@-0.0000000001' '@9223372036854775807' '@-9223372036854775808'
  '@9223372036854775808' '@1 day' '@0 + 1 day'
  # --- relative items
  'now' 'today' 'tomorrow' 'yesterday' 'TOMORROW' 'tomorrow 12:00'
  '1 day' '1 day ago' '+1 day' '-1 day' '- 1 day' '+ 1 day' '1 day ago 2 hours'
  '1 day 2 hours ago' '2 weeks' 'fortnight' '1 fortnight ago' '3 months'
  '1 year ago' '100 years' 'next week' 'last week' 'this week' 'next month'
  'last year' 'third day' 'twelfth month' 'next hour' '90 minutes'
  '1.5 seconds' '-1.5 seconds' '1,5 seconds' '1 sec' '2 secs' '2 mins'
  '3 hours hence' '1 day ago ago' '2021-01-31 1 month' '2021-03-31 1 month ago'
  '2020-02-29 1 year' '2021-06-15 12:00:00 1 day'
  # --- weekdays
  'monday' 'Mon' 'mon.' 'tues' 'wednes' 'thur' 'next monday' 'last monday'
  'this monday' 'third monday' '2 monday' 'monday 12:00' 'monday,'
  'Tue 2021-06-15' 'sunday next week' 'monday tuesday' 'Blursday'
  # --- the twelve-hour clock
  '12am' '12pm' '12:30am' '12:30:45 pm' '1 a.m.' '11 P.M.' '13pm' '0am'
  '12 noon' 'noon' 'midnight'
  # --- a signed number after a time is a zone
  '12:30 -5' '12:30 +5:30' '12:30 +0530' '12:30 +24' '12:30 +2401'
  '12:30 -1 day' '2021-06-15 12:00:00 +1 day' '2021-06-15 12:00:00 -1 day'
  '2021-06-15 12:00:00 UTC +1 day' '2021-06-15 12:00:00 -90 seconds'
  # --- zone names
  'EST' 'EDT' 'CST' 'PDT' 'GMT' 'UT' 'UTC' 'Z' 'J' 'T' 'A' 'M' 'N' 'Y' 'IST'
  'NZDT' 'BST' 'CEST' 'MSK' 'WET' 'E.S.T.' 'e.s.t' 'EST DST' 'CET DST' 'LHDT'
  'LHST' '2021-06-15 12:00 EST' '2021-06-15 12:00 EDT' '2021-01-15 12:00 EDT'
  '2021-06-15 12:00 GMT+2' '2021-06-15 12:00 UTC-5:30' '2021-06-15 12:00 EST+1'
  'UTC+14' 'UTC+25' '2021-06-15 12:00 EST EDT' '2021-06-15 12:00 J'
  '2021-06-15 T' '12:00 T 1 day'
  # --- a zone for the rest of the string
  'TZ="UTC0" 12:00' 'TZ="America/New_York" 2021-06-15 12:00'
  'TZ="Asia/Tokyo" 9am tomorrow' 'TZ="EST5" 2021-06-15 12:00'
  'TZ="EST\5" 12:00' 'TZ="EST5' 'TZ="a\"b" 12:00' '  TZ="UTC0"12:00'
  'TZ="UTC0" TZ="UTC0" 12:00'
  # --- daylight saving's edges
  '2021-03-14 02:30' '2021-03-14 02:30 EST' '2021-03-14 02:30 -0500'
  '2021-03-14 02:30 EDT' '2021-11-07 01:30' '2021-11-07 01:30 EST'
  '2021-11-07 01:30 EDT' '2021-11-07 01:30 -0500' '2021-03-13 02:30 1 day'
  '2021-03-14 1 day ago' '2021-04-04 01:45' '2021-10-03 02:15'
  '2021-10-03 02:15 1 hour'
  # --- refusals
  '' ' ' 'junk' 'epoch' '1 banana' 'next banana' '2021-02-29' '2021-02-30'
  '2021-13-01' '2021-00-10' '2021-06-00' '2021-06-31' '25:00' '24:00' '23:60'
  '23:59:60' '23:59:61' '12:30:45.5.5' '1:2:3:4' '2021-06-15 2021-06-16'
  '12:00 13:00' 'EST EDT'
  # --- comments
  '(comment) 2021-06-15' '2021-06-15 (unterminated' '((nested) comment) 12:00'
  ')' '(' '12:00 (a) (b)'
  # --- overflow, and C's integer rules
  '99999999999999999999' '2021-06-15 99999999999999999999 seconds'
  '9223372036854775807 seconds' '1 day 9223372036854775807 seconds'
  '2147483647 years' '2147483648 years' '-2147483649 years' '10:4294967326'
  '4294967298:00' '2021-06-15 12:00 4294967296 minutes'
  # --- bytes the grammar has no use for, and signs with nothing after
  $'J\xc3\xbcne 15' $'2021-06-15 12:00 \xc3\x86ST' '+' '-' '- -' '+-5' '--5 days'
  'a very long word indeed' 'e.............................st'
)

# `touch -r REF -d STRING` on one side, and the two times it left, read with
# the one GNU `stat`: "rc|stderr|atime|mtime".
touch_once() {
  local side=$1 tz=$2 ref=$3 s=$4 out rc err times
  out=$work/out-$side
  rm -f "$out"
  err=$( { timeout -k 2 20 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/$side" \
             touch -r "$ref" -d "$s" "$out"; } 2>&1 >/dev/null )
  rc=$?
  times=
  [ -e "$out" ] && times=$(TZ=UTC0 stat -c '%.9X|%.9Y' "$out")
  printf '%s|%s|%s' "$rc" "$err" "$times"
}

# `date ARGS...` on one side: "rc|stdout|stderr", with `--debug`'s
# nanoseconds folded, since those are the clock's. Standard input is
# `$DATE_STDIN`, opened afresh for each run, since each side must read all of
# it.
date_once() {
  local side=$1 tz=$2; shift 2
  local out err rc
  out=$work/dout-$side; err=$work/derr-$side
  timeout -k 2 20 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/$side" date "$@" \
    <"${DATE_STDIN:-/dev/null}" >"$out" 2>"$err"
  rc=$?
  printf '%s|%s|%s' "$rc" "$(cat "$out")" \
    "$(sed -E 's/^(date: final: [-0-9]+)\.[0-9]{9} /\1.NNNNNNNNN /' "$err")"
}

# `known_bug KEY LABEL`: a difference nobody wants, written up in
# `known-issues.md` under KEY and not yet fixed. It is printed with its key and
# does not fail the run -- until it stops differing, which does, because a
# marker that outlives its bug is a false statement about the tree.
declare -A KNOWN=()
known_bug() { KNOWN[$2]=$1; }

verdict() {
  local label=$1 o=$2 g=$3 key=${KNOWN[$1]:-}
  if [ -n "$key" ]; then
    if [ "$o" = "$g" ]; then
      kfixed=$((kfixed+1))
      printf 'KFIXED %s  (%s no longer reproduces -- close it and drop the marker)\n' "$label" "$key"
    else
      kbug=$((kbug+1))
      printf 'KBUG %s  (%s)\n  ours: %s\n  gnu : %s\n' "$label" "$key" \
        "$(printf '%s' "$o" | tr '\n' '|')" "$(printf '%s' "$g" | tr '\n' '|')"
    fi
    return 0
  fi
  if [ "$o" = "$g" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n  ours: %s\n  gnu : %s\n' "$label" \
      "$(printf '%s' "$o" | tr '\n' '|')" "$(printf '%s' "$g" | tr '\n' '|')"
  fi
  return 0
}

# A `date` row: compared, and if different, run once more, since a second
# boundary between the two sides is a difference in the clock and not in
# either program.
date_case() {
  local tz=$1; shift
  local o g
  o=$(date_once ours "$tz" "$@"); g=$(date_once gnu "$tz" "$@")
  if [ "$o" != "$g" ]; then
    o=$(date_once ours "$tz" "$@"); g=$(date_once gnu "$tz" "$@")
  fi
  verdict "TZ=$tz date $*" "$o" "$g"
}

echo "parse-datetime-diff:"
echo "  ours: $bindir/ours"
echo "  gnu:  $bindir/gnu"

# --- the REF files: GNU `touch` stamps them, so both sides start alike ------
ref_files=()
for i in "${!REFS[@]}"; do
  f=$work/ref$i
  : >"$f"
  env TZ=UTC0 "$bindir/gnu/touch" -d "${REFS[$i]}" "$f"
  ref_files+=("$f")
done

# --- 1. every string, relative to every REF, in every zone -----------------
for tz in "${ZONES[@]}"; do
  for i in "${!ref_files[@]}"; do
    for s in "${CORPUS[@]}"; do
      verdict "TZ=$tz touch -r <${REFS[$i]}> -d '$s'" \
        "$(touch_once ours "$tz" "${ref_files[$i]}" "$s")" \
        "$(touch_once gnu "$tz" "${ref_files[$i]}" "$s")"
    done
  done
done

# --- 2. `date -d`, rendered by the zone ------------------------------------
# No year in the rendering: `%s` already pins the instant, and how a year of 21
# or of 10000 is *printed* is `strftime`'s business, which `date-diff.sh` checks.
for tz in "${ZONES[@]}"; do
  for s in "${CORPUS[@]}"; do
    date_case "$tz" -d "$s" '+%s|%m-%d %T %Z %z'
  done
done

# --- 3. `--debug`: every part, every warning, in the tables' order ----------
for tz in UTC0 America/New_York; do
  for s in "${CORPUS[@]}"; do
    date_case "$tz" --debug -d "$s" '+%s'
  done
done
date_case UTC0 --debug -d 1 -d 2 +%s
# `-s` asks to set the clock, which an ordinary user may not: both sides must
# say so, and print the date anyway. Never as root, where it would succeed.
if [ "$(id -u)" != 0 ]; then
  date_case UTC0 --debug -s @0 -s @1 +%s
  date_case UTC0 -s '2021-06-15 12:00' +%s
fi

# --- 4. `date -f`: the corpus as one process ----------------------------------
# What only one process shows is order: `mktime` starts each search from the
# offset the previous call found, so which of a repeated hour's two instants
# line 7 gets depends on lines 1-6, in ours as in glibc's.
#
# A file of lines takes long enough to convert that a second boundary falls
# inside it as often as not, so the lines that read the clock to the second
# (`now`, `90 minutes`, `tomorrow`) cannot be compared here. GNU sorts them
# out itself: they are the ones whose answer carries the clock's nanoseconds.
# The rest are absolute, or relative only to today's date. Then the awkward
# lines a file can hold and argv cannot: a NUL, a lone carriage return, and no
# final newline.
lines=$work/lines
: >"$lines"
for s in "${CORPUS[@]}"; do
  ns=$(env TZ=UTC0 "$bindir/gnu/date" -d "$s" +%N 2>/dev/null) || ns=refused
  case $ns in
    000000000 | refused) printf '%s\n' "$s" >>"$lines" ;;
  esac
done
printf '2021-06-15\0junk\n12:00\r\nlast line without a newline' >>"$lines"
# Not the POSIX rule: see the known bug above, which a whole file would repeat.
for tz in UTC0 America/New_York Australia/Lord_Howe Asia/Kolkata; do
  date_case "$tz" -f "$lines" '+%s|%m-%d %T %Z'
done
date_case America/New_York --debug -f "$lines" '+%s'
DATE_STDIN=$lines date_case UTC0 -f - '+%s'

# The wording is the family's: `all-diff.sh` decides green by matching
# " 0 differed" in this line.
printf '\nparse-datetime: %d passed, %d differed, %d known bugs' "$pass" "$fail" "$kbug"
[ "$kfixed" -gt 0 ] && printf ', %d known bugs are FIXED (close them)' "$kfixed"
printf '\n'
[ "$fail" -eq 0 ] && [ "$kfixed" -eq 0 ]
