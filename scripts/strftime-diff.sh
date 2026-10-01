#!/usr/bin/env bash
# strftime-diff.sh — both of GNU's time formatters, ours against theirs.
#
# ## What this is checking
#
# `localtime` carries two ports (see its `strftime` module): gnulib's
# `nstrftime`, which coreutils' `date`, `ls`, `stat`, `du`, `pr` and diffutils
# format with, and glibc's `strftime`, which `find -printf`, `ps`, `tar`,
# `pinky` and the shell call. They disagree with each other on purpose -- year
# padding, `%N`, `%q`, `%:z`, the `+` flag, what `-` does to a width -- so each
# is checked against its own upstream:
#
# | formatter | ours | theirs |
# |---|---|---|
# | gnulib `nstrftime` | our `date -f INSTANTS +FORMAT` | GNU 9.4's, built from source |
# | glibc `strftime` | `examples/strftime-probe.rs` | `scripts/strftime-probe.c`, built here against WSL's glibc |
#
# The formats are every conversion letter (and `%:z` through `%::::z`) under
# every flag, width and modifier combination below, over instants chosen for
# what they stress: a year of 21 and of 10000 and below zero, a Sunday, the
# last half second of a year, New York's repeated hour, and offsets with
# minutes, with a DST half hour, and of "-00".
#
# `date` is given many formats per run, joined by a separator, rather than one
# run per format: each directive is formatted independently of its
# neighbours, and a run per format would be tens of thousands of processes.
#
# ## Cases that differ on purpose
#
# None.
set -u

# `DIFF_PROG` is the name both sides are run by -- `date`, whose reference is
# the built 9.4 -- not this harness's subject: the preamble names the PATH
# links after it. `DIFF_BINS` is spelled out because `DIFF_EXAMPLES` would
# otherwise empty it.
DIFF_PROG='date'
DIFF_BINS='date'
DIFF_EXAMPLES='strftime-probe'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='gcc timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0
work=$DIFF_TMP/work
mkdir -p "$work"

if ! gcc -O2 -o "$work/probe" "$root/scripts/strftime-probe.c"; then
  echo "strftime-diff: could not build the C probe" >&2
  exit 1
fi
ours_probe=$(diff_ours_example strftime-probe)

ZONES=(UTC0 America/New_York Asia/Kolkata Australia/Lord_Howe '<-00>0')

# Seconds, and the nanoseconds `date` gets with them.
INSTANTS=(
  1000000000.123456789
  0.000000000
  -1.000000000
  1609459199.500000000
  1636263000.000000042
  -61490188800.000000000
  253402300800.999999999
  -62167219200.000000000
  -62198755200.000000007
)

CONVS=(a A b B c C d D e F g G h H I j k l m M n N p P q r R s S t T u U V w W x X y Y z Z % Q ':z' '::z' ':::z' '::::z' ':')
FLAGS=('' - _ 0 '^' '#' + -0 0- '^#' '_^')
WIDTHS=('' 1 5 12)
MODS=('' E O)

formats=()
for c in "${CONVS[@]}"; do
  for fl in "${FLAGS[@]}"; do
    for w in "${WIDTHS[@]}"; do
      for m in "${MODS[@]}"; do
        formats+=("%$fl$w$m$c")
      done
    done
  done
done
# A few that are about the format rather than a conversion: a `%` at the end,
# a width with nothing after it, and literal text around directives.
formats+=('%' 'abc%' '%5' '%E' '%^' 'a%Yb%mc' '100%' '%%%' '%5%d' '%-' '%10%')

verdict() {
  local label=$1 o=$2 g=$3
  if [ "$o" = "$g" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n  ours: %s\n  gnu : %s\n' "$label" \
      "$(printf '%s' "$o" | od -An -c | tr -s ' \n' ' ')" \
      "$(printf '%s' "$g" | od -An -c | tr -s ' \n' ' ')"
  fi
  return 0
}

echo "strftime-diff: ${#formats[@]} formats, ${#INSTANTS[@]} instants, ${#ZONES[@]} zones"

# --- 1. gnulib's nstrftime, through `date` ----------------------------------------
# Both sides must actually start. A `date` missing from either PATH fails as
# "not found" on that side, and missing from both, the two failures agree and
# every chunk below "passes" -- which is how this section's first run went.
for side in ours gnu; do
  if ! env PATH="$bindir/$side" date +%s >/dev/null 2>&1; then
    echo "strftime-diff: no working \`date\` in $bindir/$side" >&2
    exit 1
  fi
done

instants=$work/instants
: >"$instants"
for t in "${INSTANTS[@]}"; do printf '@%s\n' "$t" >>"$instants"; done

sep=$(printf '\037|')
chunk=150
for tz in "${ZONES[@]}"; do
  i=0
  while [ "$i" -lt "${#formats[@]}" ]; do
    joined=
    for f in "${formats[@]:$i:$chunk}"; do
      joined=$joined$f$sep
    done
    o=$( { timeout -k 2 30 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/ours" date -f "$instants" "+$joined"; echo "rc=$?"; } 2>&1 | od -An -c)
    g=$( { timeout -k 2 30 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/gnu" date -f "$instants" "+$joined"; echo "rc=$?"; } 2>&1 | od -An -c)
    if [ "$o" = "$g" ]; then
      pass=$((pass+1))
    else
      # Narrow the difference to single formats before reporting it.
      for f in "${formats[@]:$i:$chunk}"; do
        verdict "TZ=$tz date -f instants +'$f'" \
          "$( { timeout -k 2 30 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/ours" date -f "$instants" "+$f"; echo "rc=$?"; } 2>&1)" \
          "$( { timeout -k 2 30 env TZ="$tz" LC_ALL=C.UTF-8 PATH="$bindir/gnu" date -f "$instants" "+$f"; echo "rc=$?"; } 2>&1)"
      done
    fi
    i=$((i + chunk))
  done
done

# --- 2. glibc's strftime, through the probes --------------------------------------
cases=$work/cases
: >"$cases"
for t in "${INSTANTS[@]}"; do
  for f in "${formats[@]}"; do
    printf '%s\t%s\n' "${t%%.*}" "$f" >>"$cases"
  done
done
for tz in "${ZONES[@]}"; do
  env TZ="$tz" LC_ALL=C.UTF-8 "$work/probe" <"$cases" >"$work/theirs"
  env TZ="$tz" LC_ALL=C.UTF-8 "$ours_probe" <"$cases" >"$work/ours"
  if cmp -s "$work/ours" "$work/theirs"; then
    pass=$((pass+1))
  else
    paste -d '\n' "$cases" "$work/ours" "$work/theirs" | awk -v tz="$tz" '
      NR % 3 == 1 { c = $0 } NR % 3 == 2 { o = $0 }
      NR % 3 == 0 && o != $0 { printf "DIFF TZ=%s strftime %s\n  ours: %s\n  gnu : %s\n", tz, c, o, $0; n++ }
      END { exit n > 0 }' || fail=$((fail+1))
  fi
done

# The wording is the family's: `all-diff.sh` decides green by matching
# " 0 differed" in this line.
printf '\nstrftime: %d passed, %d differed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
