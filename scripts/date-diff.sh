#!/usr/bin/env bash
# Differential test: our `date` against GNU date.
#
# ## Why there are no "now" cases in here
#
# `date` with no arguments prints the current time, and the two sides run a few
# milliseconds apart. Most of the time they agree; when the second ticks between
# them they do not, and the harness reports a difference that is a property of
# the clock rather than of either program. A gate that fails once an hour for a
# reason nobody can reproduce is a gate that gets switched off.
#
# So every case below is a function of its ARGUMENTS: `-d @<epoch>`, `-d <fixed
# string>`, `-r <file with a fixed mtime>`. That is not a gap in coverage of the
# formatting, which is the part worth comparing -- `date -d @0 +%F` exercises the
# same formatter as `date +%F` and can be checked. What it does not cover is
# reading the clock, and reading the clock is one line.
#
# `TZ` is pinned to UTC for both sides, for the same reason: an unpinned zone
# makes every `%Z` and every local-time rendering depend on the host.
#
# ## What the pair looks like going in
#
# `userspace/coreutils/src/bin/date.rs` is 257 lines whose header says "Usage:
# date -- prints the current UTC date and time in a simple format. (No timezone
# support yet -- always UTC.)" It parses **no arguments at all**. The standalone
# carries `-d`, `-r`, `-s`, `-u`, `-R`, `-I` and a `--json` of its own.
#
# So this is expected to be the second pair after `diff` where the standalone is
# the better half -- and §1005 resolves that by PORTING into `coreutils`, not by
# deleting. Expected, but measured anyway: five predictions from option counts
# have been wrong in this tree, twice inverted. The numbers are what decides.
#
# ## Why the reference is built rather than installed
#
# `date` is GNU coreutils, so §726 applies and `DIFF_GNU_SOURCE` builds 9.4 from
# source rather than trusting Ubuntu's patched `9.4-3ubuntu6.3`.
#
# ## Why `od -An -c`
#
# `-I`/`--iso-8601` and `--rfc-3339` differ from each other by one character in
# the middle (`T` against a space), `%n` and `%t` emit a newline and a tab, and
# `+%_d` pads with a space where `+%-d` does not pad at all. Every one of those
# is invisible to a comparison that trims.
set -u

DIFF_PROG='date'
DIFF_GNU_SOURCE=9.4
# Every invocation is bounded, both sides. `date -d` accepts a small language,
# and a parser for a small language is a thing that can loop on input it does
# not expect -- several cases below are exactly such input.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# A file with a fixed mtime, for `-r`. Stamped rather than created-and-hoped:
# `-r` prints the file's time, so the time has to be one this harness chose.
printf 'x\n' > stamped.txt
touch -d '2020-01-02 03:04:05 UTC' stamped.txt

# A file of dates, for `-f`.
printf '@0\n@1000000000\n2021-03-04 05:06:07\n' > dates.txt
printf '' > empty.txt

run_side() {
  local side=$1; shift
  diff_run timeout -k 2 15 env TZ=UTC LC_ALL=C.UTF-8 PATH="$bindir/$side" date "$@"
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

run_case() { compare "$@"; report "date $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS date %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail date %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- a fixed instant, rendered every documented way ------------------------------
run_case -d @0
run_case -d @1000000000
run_case -d @1700000000
run_case -d @-1
run_case --date=@0
run_case -u -d @0
run_case --utc -d @0
run_case --universal -d @0
run_case --uct -d @0

# --- the format string, which is where the surface actually is ---------------------
for f in %Y %m %d %H %M %S %F %T %D %R %e %j %u %w %a %A %b %B %C %g %G %V %U %W \
         %n %t %% %s %Z %z %:z %::z %p %P %I %k %l %N %q %x %X %c %h %r; do
  run_case -d @1000000000 "+$f"
done

# --- padding and case modifiers ------------------------------------------------------
run_case -d @1000000000 '+%-d'
run_case -d @1000000000 '+%_d'
run_case -d @1000000000 '+%0e'
run_case -d @1000000000 '+%^a'
run_case -d @1000000000 '+%#a'
run_case -d @1000000000 '+%5S'
run_case -d @1000000000 '+%-5S'

# --- whole formats -------------------------------------------------------------------
run_case -d @1000000000 '+%Y-%m-%dT%H:%M:%S'
run_case -d @1000000000 '+literal text'
run_case -d @1000000000 '+'
run_case -d @1000000000 '+%'
run_case -d @1000000000 '+%Q'
run_case -d @1000000000 '+a%Yb%mc'

# --- the canned formats -----------------------------------------------------------------
run_case -R -d @1000000000
run_case --rfc-email -d @1000000000
run_case --rfc-822 -d @1000000000
run_case --rfc-2822 -d @1000000000
run_case -I -d @1000000000
run_case --iso-8601 -d @1000000000
run_case -Idate -d @1000000000
run_case -Ihours -d @1000000000
run_case -Iminutes -d @1000000000
run_case -Iseconds -d @1000000000
run_case -Ins -d @1000000000
run_case --iso-8601=seconds -d @1000000000
run_case --rfc-3339=date -d @1000000000
run_case --rfc-3339=seconds -d @1000000000
run_case --rfc-3339=ns -d @1000000000
run_case --rfc-3339 -d @1000000000

# --- the -d language, which is the largest thing here --------------------------------------
run_case -d '2021-03-04 05:06:07'
run_case -d '2021-03-04T05:06:07'
run_case -d '2021-03-04'
run_case -d '05:06:07'
run_case -d 'Mar 4 2021'
run_case -d '4 March 2021'
run_case -d '2021-03-04 05:06:07 UTC'
run_case -d '2021-03-04 05:06:07 +0200'
run_case -d '1970-01-01 00:00:00 UTC'
run_case -d 'epoch'
run_case -d 'now'
run_case -d 'today'
run_case -d 'tomorrow'
run_case -d 'yesterday'
run_case -d '@0 + 1 day'
run_case -d 'not a date at all'
run_case -d ''
run_case -d

# --- reading a file's time -------------------------------------------------------------------
run_case -r stamped.txt
run_case --reference=stamped.txt
run_case -r stamped.txt '+%F %T'
run_case -r /nosuch
run_case -r

# --- reading dates from a file ------------------------------------------------------------------
run_case -f dates.txt
run_case --file=dates.txt
run_case -f empty.txt
run_case -f /nosuch
run_case -f

# --- refusals -------------------------------------------------------------------------------------
run_case -Q
run_case --nosuchoption
run_case -d @0 extra
run_case '+%F' extra
run_case --set
run_case -d @0 -r stamped.txt

# --- long-option abbreviation -----------------------------------------------------------------------
run_case --dat=@0
run_case --ref=stamped.txt
run_case --iso
run_case --rfc
run_case --u

# --- the two whose text is ours -----------------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
