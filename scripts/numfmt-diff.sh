#!/usr/bin/env bash
# numfmt-diff.sh — compare our `numfmt` against GNU's, inside WSL.
#
# ## What this is checking
#
# `numfmt`'s digits come out of x87 `long double` arithmetic -- scaling by
# 1000 or 1024, rounding through `(intmax_t)` casts, `%.*Lf` -- so most cases
# here are numbers chosen to land on the boundaries where that arithmetic
# decides what is printed:
#
#   * **`--to`** -- each scale, every rounding method, values just under and
#     over each power (`999`, `999.95`, `1023`, `1024`), negatives, fractions,
#     the 999Q ceiling;
#   * **`--from`** -- each scale, every suffix letter, `i` where it is required
#     and where it is refused, unit sizes (`K`, `Ki`, `KiB`);
#   * **`--format`, `--padding`, `--grouping`** -- the directive grammar and
#     every refusal of it, including upstream's `%%` accounting;
#   * **input** -- fields and delimiters, automatic padding, `--header`,
#     `-z`, `--suffix`, and each `--invalid` mode over a stream with bad lines
#     in it;
#   * **`--debug`** warnings, and a write error.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
# `---debug` (upstream's undocumented developer trace) is accepted but the
# trace is not written.
set -u

DIFF_PROG='numfmt'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
INPUT=/dev/null

fx=$DIFF_TMP/fx
mkdir -p "$fx" || exit 1
printf '1000\n2000000\nabc\n3072\n' > "$fx/mixed"
printf 'Size Name\n1024 a\n2048000 b\n' > "$fx/header"
printf '  1000   x\n 20000  y\n300000 z\n' > "$fx/columns"
printf '1000\0002048\0' > "$fx/nul"
printf 'a:1000:b\nc:2048:d\n' > "$fx/colon"
printf '1KB\n2MB\n' > "$fx/suffixed"
printf '5\n' > "$fx/five"

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" numfmt "$@" ) <"$INPUT" >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" numfmt "$@" ) <"$INPUT" >"$out" 2>"$err"
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
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf 'numfmt %s%s%s' "$*" "${INPUT:+ <$(basename "$INPUT")}" "${TO_FULL:+  [>/dev/full]}"
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
  TO_FULL=; INPUT=/dev/null
  return 0
}

xfail_case() {
  local why=$1; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  TO_FULL=; INPUT=/dev/null
  return 0
}

# --- --to: each scale over the boundaries ------------------------------------------
for scale in si iec iec-i; do
  run_case --to=$scale 0 1 9 10 99 100 999 1000 1001 1023 1024 1025 9999 10000
  run_case --to=$scale 99999 100000 999499 999500 999999 1000000 1048575 1048576
  run_case --to=$scale 123456789 9876543210 1234567890123 999999999999999999
  run_case --to=$scale -- -1 -999 -1000 -1024 -123456 -999999
  run_case --to=$scale 0.5 1.5 9.95 9.949 999.95 1023.99 12.345
done
for round in up down from-zero towards-zero nearest; do
  run_case --to=si --round=$round 1001 1499 1500 1501 1999 9949 9950 9951 123456
  run_case --to=iec --round=$round -- -1001 -1500 -2047 -2048 -123456
done
run_case --to=si 999000000000000000000000000000000
run_case --to=si 9999999999999999999999999999999999
run_case --to=si 1000000000000000000000000000000000000
run_case 123456789012345678
run_case 1234567890123456789
run_case 1.23456789012345678
run_case --format=%.20f 1
run_case --to=si --format=%.3f 1234567 999999

# --- --from: scales, suffixes, units -------------------------------------------------
for scale in auto si iec iec-i; do
  run_case --from=$scale 1K 1M 1G 1T 1P 1E 1Z 1Y 1R 1Q
done
run_case --from=auto 1Ki 1Mi 2.5Ki 1.5M 1KiB
run_case --from=iec-i 1Ki 2Mi
run_case --from=iec-i 1024
run_case --from=si 1k
run_case --from=si 1KB
run_case --from=none 1K
run_case 1K
run_case --from=si 1.5K 0.5M 1.25G -- -1K
run_case --from-unit=512 4
run_case --from-unit=K 4
run_case --from-unit=Ki 4
run_case --from-unit=1Ki 4
run_case --from-unit=KiB 4
run_case --from-unit=0 4
run_case --from-unit=x 4
run_case --to-unit=1024 1048576
run_case --to-unit=K --to=si 5000000
run_case --from=iec --to=si 1M
run_case --from=si --to=iec 1M

# --- --format, --padding, --grouping -------------------------------------------------
run_case --format=%f 1234.5
run_case --format=%.2f 1234.5
run_case --format=%.0f 1234.5
run_case --format=%.f 1234.5
run_case --format=%010f 42
run_case --format=%010.3f 42
run_case --format=%-10f 42
run_case --format="%'f" 1234567
run_case --format="%'10f" 1234567
run_case --format="% 10f" 42
run_case --format="%0 10f" 42
run_case --format=%+10f 42
run_case --format="abc%fdef" 42
run_case --format="%%%f" 42
run_case --format="a%%b%f" 42
run_case --format="%f%%" 42
run_case --format="%f%%x" 42
run_case --format=%
run_case --format=abc
run_case --format=%d
run_case --format=%f%f
run_case --format=%f%
run_case --format=%.-1f 1
run_case --format="%. 1f" 1
run_case --format=%.+1f 1
run_case --format=%99999999999999999999f 1
run_case --format=%.99999999999999999999f 1
run_case --format=%0200f 1
run_case --format=%10f --padding=5 42
run_case --to=si --format=%5f 1000
run_case --padding=10 42
run_case --padding=-10 42
run_case --padding=0 42
run_case --padding=x 42
run_case --padding=5 1234567
run_case --grouping 1234567
run_case --grouping --to=si 1234567
run_case --grouping --format=%f 1

# --- input: fields, delimiters, header, -z, suffixes ------------------------------------
run_case --to=si "1000 2000 3000"
run_case --field=2 --to=si "1000 2000 3000"
run_case --field=2- --to=si "1000 2000 3000"
run_case --field=-2 --to=si "1000 2000 3000"
run_case --field=- --to=si "1000 2000 3000"
run_case --field=1,3 --to=si "1000 2000 3000"
run_case --field=0 1
run_case --field=2 --field=3 1
run_case --field=x 1
run_case -d: --field=2 --to=si "a:1000:b"
run_case -d '' --field=1 1000
run_case -d ab 1
run_case -d, --to=si "1000,,2000"
INPUT=$fx/columns; run_case --to=si
INPUT=$fx/columns; run_case --to=si --field=1
INPUT=$fx/columns; run_case --padding=8 --to=si
INPUT=$fx/header; run_case --header --field=1 --to=iec
INPUT=$fx/header; run_case --header=2 --to=iec
INPUT=$fx/header; run_case --header=0
INPUT=$fx/header; run_case --header=x
run_case --header --debug 5
INPUT=$fx/nul; run_case -z --to=iec
INPUT=$fx/colon; run_case -d: --field=2 --to=iec
INPUT=$fx/suffixed; run_case --suffix=B --from=si
INPUT=$fx/suffixed; run_case --suffix=B --from=si --to=iec
run_case --suffix=B --to=si 1000
run_case --suffix=XY 5XY
run_case -- -5
run_case 5.
run_case 5..
run_case ''
run_case ' 5'

# --- --invalid over a stream with bad lines -----------------------------------------------
for mode in abort fail warn ignore; do
  INPUT=$fx/mixed; run_case --invalid=$mode --to=si
done
run_case --invalid=warn --to=si 1000 x 2000
run_case --invalid=fail x
run_case --invalid=bogus x
run_case --to=bogus 1
run_case --round=nope 1
run_case --to=s 1000
run_case --round=n 1500

# --- --debug ----------------------------------------------------------------------------
run_case --debug 5
run_case --debug --grouping 5
run_case --debug --to=si 12345678901234567890
run_case --debug --invalid=warn x
run_case --debug --padding=5 --format=%10f 42

# --- the command line and a write error ---------------------------------------------------
run_case -x
run_case --nope
run_case --from
run_case --grouping=1 5
INPUT=$fx/five; run_case
TO_FULL=1; run_case --to=si 1000
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case "upstream's developer trace is not ported" ---debug 5

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
