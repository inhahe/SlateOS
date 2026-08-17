#!/usr/bin/env bash
# Differential test: our awk against the host's GNU awk.
#
# Each case is `run_case INPUT-NAME ARGS...`. Both awks get identical argv and
# identical stdin; stdout and the exit status are compared byte for byte. This
# is the check a unit test cannot make, because a unit test asserts what we
# believe awk does rather than what awk does.
#
# `xfail_case REASON INPUT-NAME ARGS...` is a case where we differ deliberately
# and the reason is written down — see the head of awk's `main.rs` for the list.
# The script fails if a plain case differs, and also if an xfail *stops*
# differing, because that means the recorded reason no longer describes reality.
#
# stderr only has to agree about *whether* there was a diagnostic. Matching
# gawk's wording exactly would be fitting to its parser's internals, not to awk.
#
# gawk is not POSIX awk out of the box — it has `gensub`, `\y`, `RT`,
# `IGNORECASE` and a longer list besides — so it is run with `--posix`, which is
# the dialect we are actually claiming to implement.
set -u

# Our awk is a native Windows binary, so MSYS would helpfully rewrite an
# argument that looks like a path — turning the program `$1 ~ /^\//` into
# something with a drive letter in it.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/awk.exe"}
GNU=${GNU:-gawk}
GNUFLAGS=${GNUFLAGS:---posix}

declare -A INPUTS=(
  [abc]=$'a\nb\nc\n'
  [nums]=$'1\n2\n3\n4\n5\n'
  [table]=$'alice 30 red\nbob 25 blue\ncarol 41 red\ndave 25 green\n'
  [csv]=$'a:b:c\nd:e:f\n:g:\n'
  [blanks]=$'a\n\n\n\nb\n\nc\n'
  [spaces]=$'  lead\ttab  \n one \n'
  [nonl]=$'a\nb'
  [empty]=''
  [mixed]=$'Alpha1\nbeta22\nGAMMA333\n'
  [strnum]=$'0\n00\n0.0\n1e3\n abc\n+7\n'
  [para]=$'one\ntwo\n\n\nthree\nfour\n'
)

pass=0; fail=0; xfail=0; xpass=0

# Both awks, same argv and stdin. Sets AGREED=yes/no and the REPORT text.
compare() {
  local input_name="$1"; shift
  local data="${INPUTS[$input_name]}"

  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$(printf '%s' "$data" | "$OURS" "$@" 2>"$o_err"); o_rc=$?
  g_out=$(printf '%s' "$data" | "$GNU" $GNUFLAGS "$@" 2>"$g_err"); g_rc=$?

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_loud" = "$g_loud" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(tr '\n' '|' <"$o_err")" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(tr '\n' '|' <"$g_err")")
  rm -f "$o_err" "$g_err"
}

# `run_case INPUT ARGS...` — the two awks must agree.
run_case() {
  local input_name="$1"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   [%s] %s\n' "$input_name" "${*:2}"
  else
    fail=$((fail+1))
    printf 'DIFF [%s] %s\n%s\n' "$input_name" "${*:2}" "$REPORT"
  fi
}

# `xfail_case REASON INPUT ARGS...` — the two awks are *known* to differ, and we
# have decided our answer is the one we want. Listing these as plain differences
# would train the reader to skim the output, and deleting them would lose the
# coverage: an xfail that starts agreeing is reported too, because that means
# either gawk or we have changed and the decision needs re-reading.
xfail_case() {
  local reason="$1"; shift
  local input_name="$1"
  compare "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL[%s] %s  (%s)\n' "$input_name" "${*:2}" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS[%s] %s\n  now agrees with gawk; the divergence was: %s\n' \
      "$input_name" "${*:2}" "$reason"
  fi
}

# --- patterns and the default action ----------------------------------------
run_case abc '{print}'
run_case abc '//'
run_case abc '/b/'
run_case abc '!/b/'
run_case abc '/b/,/c/'
run_case nums '$1 > 2'
run_case nums 'NR % 2'
run_case nums 'NR == 2, NR == 4'
run_case table '$3 == "red"'
run_case table '$2 ~ /^2/'
run_case table '$2 !~ /^2/'
run_case abc 'END {print NR}'
run_case abc 'BEGIN {print "start"} {print} END {print "stop"}'
run_case empty 'BEGIN {print "b"} END {print NR}'

# --- fields -----------------------------------------------------------------
run_case table '{print $1, $3}'
run_case table '{print NF, $NF}'
run_case table '{$2 = "X"; print}'
run_case table '{$0 = "p q r"; print NF, $2}'
run_case table '{NF = 2; print; print NF}'
run_case table '{$5 = "e"; print NF; print}'
run_case table 'BEGIN {OFS = "-"} {$1 = $1; print}'
run_case table 'BEGIN {OFS = "-"} {print $1, $2}'
run_case table '{print $(NF - 1)}'
run_case spaces '{print NF; print "[" $1 "]"}'
run_case csv -F: '{print NF, $2}'
run_case csv -F : '{print $1 "|" $3}'
run_case csv 'BEGIN {FS = ":"} {print $2}'
run_case table 'BEGIN {FS = "[aeiou]"} {print NF}'
run_case abc 'BEGIN {FS = ""} {print NF, $1}'
run_case para 'BEGIN {RS = ""} {print NR ":" NF ":" $2}'
run_case csv 'BEGIN {RS = ":"} {print NR, $0}'

# --- variables, arithmetic, strnum ------------------------------------------
run_case nums '{t += $1} END {print t}'
run_case nums '{print $1 * 2, $1 / 4, $1 % 3, $1 ^ 2}'
run_case nums '{print -$1, +$1, !$1}'
run_case nums 'BEGIN {x = 5} {x -= $1} END {print x}'
run_case strnum '{print ($1 == 0), ($1 == "0"), ($1 < "1")}'
run_case strnum '{print $1 + 0}'
run_case abc 'BEGIN {print ("10" < "9"), (10 < 9)}'
run_case abc 'BEGIN {print 1/3}'
run_case abc 'BEGIN {CONVFMT = "%.2g"; x = 1/3; print x ""}'
run_case abc 'BEGIN {OFMT = "%.2f"; print 1/3}'
run_case abc 'BEGIN {print 2 ^ 3 ^ 2}'
run_case abc 'BEGIN {print 7 % 3, -7 % 3}'
run_case nums 'BEGIN {n = 0} {n++} END {print n, n++, n}'
run_case nums '{print NR, ++i, j++}'

# --- control flow -----------------------------------------------------------
run_case nums '{if ($1 > 3) print "big"; else print "small"}'
run_case nums '{i = 0; while (i < $1) i++; print i}'
run_case nums '{i = 0; do i++; while (i < 2); print i}'
run_case nums '{for (i = 1; i <= $1; i++) s = s "*"; print s; s = ""}'
run_case nums '{for (i = 1; i <= 5; i++) {if (i == 3) continue; if (i == 5) break; printf "%d", i}; print ""}'
run_case nums 'NR == 2 {next} {print}'
run_case nums 'NR == 3 {exit} {print}'
run_case nums 'NR == 3 {exit 4} {print}'
run_case nums '{print} END {exit 2}'

# --- arrays -----------------------------------------------------------------
run_case table '{c[$3]++} END {n = 0; for (k in c) n += c[k]; print n}'
run_case table '{c[$3]++} END {print c["red"], c["blue"]}'
run_case table '{a[NR] = $1} END {for (i = 1; i <= NR; i++) print a[i]}'
run_case table 'END {print ("x" in a)}'
run_case table '{a[$1, $2] = 1} END {print (("alice" SUBSEP "30") in a)}'
run_case table '{a[$1] = 1} END {delete a["bob"]; print ("bob" in a), ("dave" in a)}'
run_case table '{a[$1] = 1} END {delete a; n = 0; for (k in a) n++; print n}'

# --- functions --------------------------------------------------------------
run_case nums 'function sq(x) {return x * x} {print sq($1)}'
run_case nums 'function f(a, b) {return a b} {print f($1, "!")}'
run_case abc 'function fill(a) {a[1] = "set"} BEGIN {fill(v); print v[1]}'
run_case abc 'function r(n) {return n <= 1 ? 1 : n * r(n - 1)} BEGIN {print r(5)}'
run_case abc 'function nop() {} BEGIN {print nop() "x"}'
run_case nums 'function add(a, b,   t) {t = a + b; return t} {print add($1, 10)}'

# --- string builtins --------------------------------------------------------
run_case mixed '{print length, length($0), length("ab")}'
run_case mixed '{print substr($0, 2), substr($0, 2, 3), substr($0, 0, 3), substr($0, -1)}'
run_case mixed '{print index($0, "a"), index($0, "zz")}'
run_case mixed '{print toupper($0), tolower($0)}'
run_case mixed '{n = gsub(/[0-9]/, "#"); print n, $0}'
run_case mixed '{n = sub(/[0-9]/, "#"); print n, $0}'
run_case mixed '{sub(/[0-9]+/, "[&]"); print}'
run_case mixed '{sub(/[0-9]+/, "[\\&]"); print}'
run_case mixed '{gsub(/x*/, "-"); print}'
run_case table '{gsub(/e/, "E", $1); print}'
run_case mixed '{print match($0, /[0-9]+/), RSTART, RLENGTH}'
run_case mixed '{print match($0, /zzz/), RSTART, RLENGTH}'
run_case csv '{n = split($0, a, ":"); print n, a[1] "/" a[n]}'
run_case csv '{n = split($0, a, /[:]/); print n}'
run_case table '{n = split($0, a); print n, a[2]}'
run_case abc 'BEGIN {print sprintf("%05.2f|%-4s|", 3.5, "x")}'
run_case abc 'BEGIN {printf "%d %i %o %x %X %c %s\n", 42, 42, 8, 255, 255, 65, "s"}'
run_case abc 'BEGIN {printf "%e %E %f %g %G\n", 1234.5, 1234.5, 1234.5, 1234.5, 0.000012345}'
run_case abc 'BEGIN {printf "%*d|%-*d|%.*f\n", 5, 42, 5, 42, 2, 1.239}'
xfail_case 'a missing printf argument is empty, as in bwk and mawk; gawk --posix makes it fatal' abc 'BEGIN {printf "%s\n"}'
xfail_case '%c of a code point above 255 is that character; gawk in the C locale truncates to a byte' abc 'BEGIN {printf "%c%c\n", "abc", 9731}'

# --- numeric builtins -------------------------------------------------------
run_case abc 'BEGIN {print int(3.9), int(-3.9), int("12x")}'
run_case abc 'BEGIN {printf "%.4f %.4f %.4f\n", sin(1), cos(1), atan2(1, 1)}'
run_case abc 'BEGIN {printf "%.4f %.4f %.4f\n", exp(1), log(10), sqrt(2)}'
run_case abc 'BEGIN {srand(1); x = srand(2); print x}'

# --- output -----------------------------------------------------------------
run_case abc '{print > "/dev/stdout"}'
run_case abc 'BEGIN {ORS = "|"} {print}'
run_case table '{print $1 $2}'
run_case table '{print $1 " " $2}'
run_case nums '{printf "%s", $1} END {print ""}'
run_case abc '{print NR ": " $0}'

# --- getline ----------------------------------------------------------------
run_case nums 'NR == 1 {getline; print "got", $0} {print "main", $0}'
run_case nums 'NR == 1 {getline x; print "got", x} {print "main", $0}'
run_case abc 'BEGIN {while (("echo hi" | getline line) > 0) print "L:" line}'
run_case abc 'BEGIN {print (getline junk < "/definitely/not/here")}'

# --- command line -----------------------------------------------------------
run_case table -v 'name=bob' '$1 == name'
run_case table -v 'n=25' '$2 == n {print "num"} $2 == "25" {print "str"}'
run_case csv -F: -v 'x=1' '{print x, NF}'
run_case abc 'BEGIN {print ARGC, (ARGV[0] != "")}'
run_case abc 'BEGIN {print (ENVIRON["PATH"] != "")}'

# --- odd inputs -------------------------------------------------------------
run_case nonl '{print NR, $0}'
run_case empty '{print "never"} END {print NR}'
run_case blanks '{print NR, NF}'
run_case blanks 'NF'
xfail_case 'length counts characters; gawk in the C locale counts bytes' abc 'BEGIN {print length("héllo")}'
xfail_case 'substr counts characters; gawk in the C locale counts bytes' abc 'BEGIN {print substr("héllo", 2, 2)}'
xfail_case 'index returns a character position; gawk in the C locale returns a byte offset' abc 'BEGIN {print index("héllo", "l")}'
xfail_case 'toupper maps the whole character repertoire; gawk in the C locale maps only ASCII' abc 'BEGIN {print toupper("héllo")}'

# --- errors -----------------------------------------------------------------
run_case abc '{print'
run_case abc 'function f() {} function f() {} BEGIN {f()}'
xfail_case 'an undefined function is caught before the program runs (exit 1), not when first called (gawk: exit 2)' abc 'BEGIN {nosuch()}'
run_case abc 'BEGIN {print 1 +}'
xfail_case 'an array/scalar conflict is caught before the program runs (exit 1), not when first reached (gawk: exit 2)' abc 'BEGIN {x[1] = 1; y = x}'
xfail_case 'a built-in called with the wrong number of arguments is caught before the program runs (exit 1)' abc 'BEGIN {split("a", b, "x", "y")}'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with gawk is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
