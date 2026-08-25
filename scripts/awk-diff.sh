#!/usr/bin/env bash
# Differential test: our awk against GNU awk, both run inside WSL.
#
# Each case gives both awks identical argv and identical stdin, and compares
# stdout and the exit status byte for byte. This is the check a unit test
# cannot make, because a unit test asserts what we believe awk does rather
# than what awk does.
#
# gawk is not POSIX awk out of the box — it has `gensub`, `\y`, `RT`,
# `IGNORECASE` and a longer list besides — so it is run with `--posix`, which
# is the dialect we are actually claiming to implement.
#
# ## The case helpers
#
# | helper | stdin | stderr compared as |
# |---|---|---|
# | `run_case INPUT ARGS...`  | fixture `INPUT` | presence |
# | `msg_case INPUT ARGS...`  | fixture `INPUT` | text |
# | `file_case ARGS...`       | `/dev/null`     | presence |
# | `fmsg_case ARGS...`       | `/dev/null`     | text |
# | `wfile_case LABEL ARGS...`| `/dev/null`     | text, plus every file the program wrote |
#
# and `xfail_*` for each, taking a REASON first. An xfail that *stops*
# differing is reported too, because that means the recorded reason no longer
# describes reality.
#
# ## Why stderr is not compared as text everywhere
#
# Most of awk's diagnostics are parse errors, and gawk's are a rendering of
# its own parser's state — `cmd. line:1: ... ^ unexpected newline or end of
# string`, with a caret column counted in gawk's tokens. Matching that would
# be fitting to gawk's internals rather than to awk, and would freeze our
# parser into gawk's shape. So the default is to agree only about *whether*
# there was a diagnostic.
#
# The diagnostics that are not parser-internal — a file that will not open, a
# bad option, a missing program text — are ordinary observable behaviour, and
# those get `msg_case`, which compares the text. That comparison is only
# possible at all because both sides now run under glibc *and* because gawk
# takes its message prefix from `argv[0]`: reached through a symlink named
# `awk` it says `awk: fatal: ...`, not `gawk: fatal: ...`. `diff-wsl.sh` puts
# both binaries behind that one name for exactly this reason.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the general reasons. The particular ones here:
#
#   * **The reference was the wrong gawk.** MSYS2's gawk is a Cygwin-derived
#     build; its `| getline` runs a Windows shell, its `/dev/stdout` is
#     emulated, and its idea of a locale is not glibc's.
#
#   * **The old harness ran in the C locale, and four xfails were nothing but
#     that.** `length`, `substr`, `index` and `toupper` were recorded as
#     deliberate divergences — "gawk in the C locale counts bytes". Under
#     `C.UTF-8`, which is what `diff-wsl.sh` fixes, gawk counts characters and
#     agrees with us on all four. They were never divergences; they were the
#     harness's locale. They are ordinary cases below.
#
#   * **stdout went through `$(...)`, which strips trailing newlines.** Every
#     `ORS`, `printf`-without-newline and unterminated-last-line case was
#     therefore blind to the one byte it was about. stdout is compared as a
#     hex dump now.
#
#   * **There were no file operands at all**, because the fixtures were shell
#     strings piped in. `FILENAME`, `FNR`, `nextfile`, `var=value` operands,
#     `getline < file`, `print > file` and the whole of `ARGV` handling could
#     not be reached. They are fixture files now, and those sections exist.
#
# Run `OURS=/usr/bin/gawk ./scripts/awk-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find gawk, and put both behind the one name
# `awk` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=awk
# `command -v awk` is not good enough: on a Debian-family system `/usr/bin/awk`
# is whatever `update-alternatives` last pointed at, and mawk is a legitimate
# answer. mawk is not the reference — it has no `--posix`, no `ENVIRON`
# ordering guarantees and a different `printf` — so gawk is named outright.
DIFF_REF='/usr/bin/gawk /usr/local/bin/gawk /usr/bin/awk'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

GNUFLAGS=${GNUFLAGS:---posix}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# The fixtures are files, and the stdin cases feed those same files. One
# source of truth, so a case that reads `nums` on stdin and one that names
# `nums.txt` as an operand are looking at identical bytes.
printf 'a\nb\nc\n'                                          > abc.txt
printf '1\n2\n3\n4\n5\n'                                    > nums.txt
printf 'alice 30 red\nbob 25 blue\ncarol 41 red\ndave 25 green\n' > table.txt
printf 'a:b:c\nd:e:f\n:g:\n'                                > csv.txt
printf 'a\n\n\n\nb\n\nc\n'                                  > blanks.txt
printf '  lead\ttab  \n one \n'                             > spaces.txt
printf 'a\nb'                                               > nonl.txt
: > empty.txt
printf 'Alpha1\nbeta22\nGAMMA333\n'                         > mixed.txt
printf '0\n00\n0.0\n1e3\n abc\n+7\n'                        > strnum.txt
printf 'one\ntwo\n\n\nthree\nfour\n'                        > para.txt
printf 'x\ny\nz\n'                                          > xyz.txt
printf 'h\xc3\xa9llo\n\x80\xff raw\n'                       > bytes.txt

# Program files, for -f.
printf '{ print "P1:" $0 }\n'                               > p1.awk
printf 'END { print "P2:" NR }\n'                           > p2.awk
printf '#!/usr/bin/awk -f\nBEGIN { print "shebang" }\n'      > p3.awk

# One invocation of one side. `$1` is `ours` or `gnu`; `$2` names a fixture to
# feed on stdin, or `-` for none.
#
# The side's own directory comes first so that `awk` is the binary under test,
# but the real directories follow it: awk can run a shell (`system()`,
# `print | "cmd"`, `"cmd" | getline`), and with a one-entry PATH every such
# case degenerates into two shells agreeing that `cat` does not exist — which
# is a comparison of the harness against itself.
run_side() {
  local side=$1 stdin=$2 out=$3 err=$4; shift 4
  local flags=
  [ "$side" = gnu ] && flags=$GNUFLAGS
  if [ "$stdin" = "-" ]; then
    # shellcheck disable=SC2086
    env PATH="$bindir/$side:/usr/bin:/bin" awk $flags "$@" </dev/null >"$out" 2>"$err"
  else
    # shellcheck disable=SC2086
    env PATH="$bindir/$side:/usr/bin:/bin" awk $flags "$@" <"$stdin" >"$out" 2>"$err"
  fi
}

# Sets AGREED (stdout + status + whether stderr was loud) and AGREED_MSG (the
# same, but stderr compared as text), plus REPORT.
compare() {
  local stdin=$1; shift
  local o_out g_out o_msg g_msg o_bin g_bin o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file, not through a pipe into `od`: in `x=$(awk | od)`
  # the status recorded is od's, and `PIPESTATUS` is set inside the command
  # substitution's subshell where it cannot be read — so every failing case
  # would compare od's success against od's success and pass.
  run_side ours "$stdin" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$stdin" "$g_bin" "$g_err" "$@"; g_rc=$?
  # A hex dump, because `$(...)` strips trailing newlines and eats NULs, and
  # `ORS`, `printf` and the unterminated-last-line cases are about exactly
  # those bytes.
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  local same_out=no
  [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && same_out=yes

  AGREED=no; AGREED_MSG=no
  [ "$same_out" = yes ] && [ "$o_loud" = "$g_loud" ] && AGREED=yes
  [ "$same_out" = yes ] && [ "$o_msg" = "$g_msg" ] && AGREED_MSG=yes

  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

# `report VERDICT LABEL` — VERDICT is yes/no.
report() {
  local verdict="$1" label="$2"
  if [ "$verdict" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# `report_x VERDICT REASON LABEL` — the case is known to differ. Listing these
# as plain differences would train the reader to skim the output, and deleting
# them would lose the coverage.
report_x() {
  local verdict="$1" reason="$2" label="$3"
  if [ "$verdict" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s  (%s)\n' "$label" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS %s\n  now agrees with gawk, so this reason is stale: %s\n' \
      "$label" "$reason"
  fi
  return 0
}

# The fixture a case name refers to. `abc` is `abc.txt`; a name that is
# already a path is taken as one.
fx() { case $1 in */*|*.*) printf '%s' "$1" ;; *) printf '%s.txt' "$1" ;; esac; }

run_case()  { local i="$1"; shift; compare "$(fx "$i")" "$@"; report     "$AGREED"     "[$i] awk $*"; }
msg_case()  { local i="$1"; shift; compare "$(fx "$i")" "$@"; report     "$AGREED_MSG" "[$i] awk $*"; }
file_case() { compare - "$@"; report "$AGREED" "awk $*"; }
fmsg_case() { compare - "$@"; report "$AGREED_MSG" "awk $*"; }

xfail_case() { local r="$1" i="$2"; shift 2; compare "$(fx "$i")" "$@"; report_x "$AGREED"     "$r" "[$i] awk $*"; }
xmsg_case()  { local r="$1" i="$2"; shift 2; compare "$(fx "$i")" "$@"; report_x "$AGREED_MSG" "$r" "[$i] awk $*"; }
xfail_file() { local r="$1"; shift;          compare - "$@";            report_x "$AGREED"     "$r" "awk $*"; }
xfmsg_file() { local r="$1"; shift;          compare - "$@";            report_x "$AGREED_MSG" "$r" "awk $*"; }

# `wfile_case LABEL ARGS...` — for a program that writes files of its own.
# Each side runs in its own copy of the fixtures, and the comparison covers
# every file left behind as well as stdout, stderr and the status. Without
# this, `print > "out"` is indistinguishable from `print` discarded.
wfile_case() {
  local label="$1"; shift
  local w=$DIFF_TMP/w side
  rm -rf "$w"; mkdir -p "$w/ours" "$w/gnu"
  local o_dump g_dump o_rc g_rc o_msg g_msg o_out g_out
  for side in ours gnu; do
    cp "$fixtures"/*.txt "$fixtures"/*.awk "$w/$side/"
  done

  ( cd "$w/ours" && run_side ours - out.stdout err.stderr "$@" ); o_rc=$?
  ( cd "$w/gnu"  && run_side gnu  - out.stdout err.stderr "$@" ); g_rc=$?
  o_msg=$(cat "$w/ours/err.stderr"); g_msg=$(cat "$w/gnu/err.stderr")
  o_out=$(od -An -tx1 <"$w/ours/out.stdout"); g_out=$(od -An -tx1 <"$w/gnu/out.stdout")

  # Only the files the program created: the fixtures are identical on both
  # sides by construction, and dumping them would bury the one file that is
  # the point of the case.
  dump_new() {
    ( cd "$1" && find . -type f ! -name '*.txt' ! -name '*.awk' \
        ! -name out.stdout ! -name err.stderr | sort | while read -r f; do
        printf '=== %s\n' "$f"; od -An -tx1 "$f"
      done )
  }
  o_dump=$(dump_new "$w/ours"); g_dump=$(dump_new "$w/gnu")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] &&
     [ "$o_msg" = "$g_msg" ] && [ "$o_dump" = "$g_dump" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n    files: %s\n  gnu  (rc=%s): %s  {%s}\n    files: %s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$(printf '%s' "$o_dump" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" \
    "$(printf '%s' "$g_dump" | tr -s ' \n' ' ')")
  rm -rf "$w"
  report "$AGREED" "$label"
}

echo "awk-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real $GNUFLAGS"

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
xfail_case '\1 is a backreference, as in GNU grep -E; gawk reads it as the octal escape \001' mixed '/(.)\1/'
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

# The four cases below were recorded as deliberate divergences for years on
# the strength of "gawk in the C locale counts bytes". It does — but the C
# locale was the old harness's, not a property of gawk. Under `C.UTF-8` gawk
# counts characters and agrees with us, so these are plain cases now. They are
# kept precisely because they were once wrong: they are the regression test
# for the locale the harness runs in.
run_case abc 'BEGIN {print length("héllo")}'
run_case abc 'BEGIN {print substr("héllo", 2, 2)}'
run_case abc 'BEGIN {print index("héllo", "l")}'
run_case abc 'BEGIN {print toupper("héllo")}'
run_case abc 'BEGIN {printf "%c%c\n", "abc", 9731}'

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
# The trailing byte is the whole content of these, and it was invisible while
# stdout went through `$(...)`.
run_case abc 'BEGIN {ORS = ""} {print}'
run_case abc '{printf "%s", $0}'
run_case abc 'BEGIN {printf "no newline"}'
run_case abc 'BEGIN {ORS = "\0"} {print}'
run_case nonl '{print NR, $0}'

# --- getline ----------------------------------------------------------------
run_case nums 'NR == 1 {getline; print "got", $0} {print "main", $0}'
run_case nums 'NR == 1 {getline x; print "got", x} {print "main", $0}'
run_case abc 'BEGIN {while (("echo hi" | getline line) > 0) print "L:" line}'
run_case abc 'BEGIN {print (getline junk < "/definitely/not/here")}'
# getline from a *file*, which the old harness had no way to write.
file_case 'BEGIN {while ((getline l < "abc.txt") > 0) print "F:" l}'
file_case 'BEGIN {getline a < "abc.txt"; close("abc.txt"); getline b < "abc.txt"; print a, b}'
file_case 'BEGIN {getline a < "abc.txt"; getline b < "abc.txt"; print a, b}'
file_case 'BEGIN {print (getline x < "empty.txt")}'
file_case 'BEGIN {n = 0; while ((getline < "table.txt") > 0) n++; print n, NF, $1}'
file_case 'BEGIN {"echo piped" | getline v; print v; close("echo piped")}'

# --- file operands ----------------------------------------------------------
# None of this section could exist before: the fixtures were shell strings on
# stdin, so FILENAME was always empty and FNR always tracked NR.
file_case '{print FILENAME, FNR, NR}' abc.txt xyz.txt
file_case 'END {print FILENAME, FNR, NR}' abc.txt xyz.txt
file_case '{print}' abc.txt abc.txt
file_case '{print}' empty.txt abc.txt empty.txt
file_case 'FNR == 1 {print "head:" FILENAME}' abc.txt xyz.txt nums.txt
file_case '{print NR, $0}' nonl.txt abc.txt
file_case 'BEGIN {print ARGC; for (i = 0; i < ARGC; i++) print i, ARGV[i]}' abc.txt xyz.txt
file_case 'BEGIN {ARGV[1] = "xyz.txt"} {print}' abc.txt
file_case 'BEGIN {delete ARGV[1]} {print "read:" $0}' abc.txt xyz.txt
# A `var=value` operand is assigned when it is *reached*, not up front.
file_case '{print v, $0}' abc.txt v=set xyz.txt
file_case 'BEGIN {print "b:" v} {print v}' v=1 abc.txt
file_case '{print v}' 'v=a\tb' abc.txt
run_case abc '{print FILENAME "|" $0}' -
file_case '{print FILENAME, $0}' - abc.txt < abc.txt

# --- -f, program files ------------------------------------------------------
file_case -f p1.awk abc.txt
file_case -f p1.awk -f p2.awk abc.txt
file_case -f p2.awk -f p1.awk abc.txt
file_case -f p3.awk /dev/null
run_case abc -f p1.awk

# --- -v and the command line ------------------------------------------------
run_case table -v 'name=bob' '$1 == name'
run_case table -v 'n=25' '$2 == n {print "num"} $2 == "25" {print "str"}'
run_case csv -F: -v 'x=1' '{print x, NF}'
run_case abc 'BEGIN {print ARGC, (ARGV[0] != "")}'
run_case abc 'BEGIN {print (ENVIRON["PATH"] != "")}'
run_case abc -v 'x=a\tb\n' 'BEGIN {printf "[%s]", x}'
run_case abc -v 'x=\\' 'BEGIN {print length(x)}'
run_case abc -v 'x=' 'BEGIN {print "[" x "]", (x == "")}'
run_case abc -- '{print}'
run_case abc -F '\t' '{print NF}'
run_case spaces -F '\t' '{print NF}'
run_case abc -F 'x' -F 'y' 'BEGIN {print FS}'

# --- redirection to files ---------------------------------------------------
# `print > "out"` is indistinguishable from a discarded `print` unless the
# file is compared, which is what `wfile_case` is for.
wfile_case 'print > file'          '{print > "out"}' abc.txt
wfile_case 'print >> file'         'BEGIN {print "pre" > "out"} {print >> "out"}' abc.txt
wfile_case 'two output files'      '{print > ($0 ".out")}' abc.txt
wfile_case 'truncate once only'    '{print > "out"} END {print "last" > "out"}' abc.txt
wfile_case 'close and reopen'      '{print > "out"; close("out")}' abc.txt
wfile_case 'printf to a file'      '{printf "%s|", $0 > "out"}' abc.txt
wfile_case 'pipe to a command'     '{print | "cat > out"}' abc.txt
wfile_case 'stderr by name'        'BEGIN {print "e" > "/dev/stderr"}'

# --- odd inputs -------------------------------------------------------------
run_case empty '{print "never"} END {print NR}'
run_case blanks '{print NR, NF}'
run_case blanks 'NF'
# A byte that is not valid UTF-8 is data, not an error. gawk warns about it
# and — worse — `toupper` replaces it with U+FFFD, which is the silent data
# corruption `from_utf8_lossy` performs and this project forbids outright
# (CLAUDE.md's self-review item 7). We pass the byte through unchanged and say
# nothing, which is `design-decisions.md` §322's byte-based model.
#
# Note that the *lengths* agree: both count 5 and 6. The divergence is only
# the warning and the substitution, not the character model.
xfail_case 'an undecodable byte is data; gawk warns about it' bytes '{print NR, length($0)}'
xfail_case 'toupper passes an undecodable byte through; gawk replaces it with U+FFFD' bytes '{print toupper($0)}'
run_case bytes '/raw/ {print "hit"}'
xfail_file 'an undecodable byte in a file is data; gawk warns about it' '{print length($0)}' bytes.txt

# --- diagnostics that are not parser-internal -------------------------------
# These are ordinary observable behaviour rather than a rendering of gawk's
# parser state, so the text is compared. See the header.
fmsg_case '{print}' /definitely/not/here
fmsg_case -f /definitely/not/here.awk abc.txt
fmsg_case 'BEGIN {print (getline < "/definitely/not/here")}'
# The *stdout* half of this one matters and is checked: the contents of
# `abc.txt` must appear before the failure on the second operand, because a
# fatal error does not retract what was already printed. That was a real bug —
# `Interp::run` skipped its flush on the error path and `process::exit` runs no
# destructors, so an entire run's buffered output vanished. See `interp.rs`.
#
# The *stderr* half differs, and deliberately. gawk prefixes this one with
# `cmd. line:1:` and the first-operand case above with nothing — the location
# is left over from the rule that last ran, and points at a line that has no
# connection to the file that failed to open. Reproducing that means
# reproducing stale interpreter state, which is the one thing this harness's
# header says it will not fit itself to.
xfmsg_file 'gawk tags this with `cmd. line:1:` — the location left over from the last rule to run, which has nothing to do with the file that failed' \
  '{print}' abc.txt /definitely/not/here

# --- errors -----------------------------------------------------------------
# Parse errors: only *whether* there was a diagnostic is compared.
run_case abc '{print'
run_case abc 'function f() {} function f() {} BEGIN {f()}'
run_case abc 'BEGIN {print 1 +}'
run_case abc 'BEGIN {'
run_case abc '}'
run_case abc '/unterminated'
run_case abc 'BEGIN {x = "unterminated}'
xfail_case 'an undefined function is caught before the program runs (exit 1), not when first called (gawk: exit 2)' abc 'BEGIN {nosuch()}'
xfail_case 'an array/scalar conflict is caught before the program runs (exit 1), not when first reached (gawk: exit 2)' abc 'BEGIN {x[1] = 1; y = x}'
xfail_case 'a built-in called with the wrong number of arguments is caught before the program runs (exit 1)' abc 'BEGIN {split("a", b, "x", "y")}'
# gawk constant-folds, so a *literal* `1/0` is a compile-time `error:` and
# exit 1 — the program never runs at all, and `BEGIN {if (0) print 1/0}` fails
# too, which is a surprising thing for a fold to do. Ours is a runtime fatal,
# exit 2. The runtime case below is the one both agree on, and it is the one
# that matters: it is what a real program hits.
xfail_case 'gawk folds constant arithmetic, so a literal 1/0 is a compile error (exit 1); ours is a runtime fatal (exit 2)' \
  abc 'BEGIN {print 1/0}'
# The runtime fatals. Two verdicts are taken from each, because the two halves
# have different standing.
#
# The stdout half is checked outright, and the `print "before"` is the whole
# point of the first case: a fatal does not retract what was already printed.
# It used to — `Interp::run` was one `?`-chain that skipped its flush on the
# error path, and `process::exit` runs no destructors, so an entire run's
# buffered output vanished with nothing to say it had. This is that bug's
# regression test.
#
# The stderr half is an xfail, and this one is *our* gap rather than a
# divergence we chose. gawk says `awk: cmd. line:1: fatal: …`; we say
# `awk: fatal: …`, because our AST carries no line numbers at all — nothing
# from `lex.rs` through `parse.rs` records where a statement came from, so
# there is nothing for `interp.rs` to report. Unlike the stale `cmd. line:1:`
# on the unopenable-second-operand case above, this location is correct and
# useful: it is the line of the user's script that blew up. Tracked in
# `known-issues.md`; when the source locations land these become `msg_case`.
run_case abc 'BEGIN {x = 0; print "before"; print 1/x}'
xmsg_case 'our runtime diagnostics carry no source location; gawk prefixes `cmd. line:N:`' \
  abc 'BEGIN {x = 0; print "before"; print 1/x}'
run_case abc 'BEGIN {x = 0; print 1 % x}'
xmsg_case 'our runtime diagnostics carry no source location; gawk prefixes `cmd. line:N:`' \
  abc 'BEGIN {x = 0; print 1 % x}'
xfail_case 'an array/scalar conflict is caught before the program runs (exit 1), not when first reached (gawk: exit 2)' abc 'BEGIN {x = 1; x[2] = 3}'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with gawk is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
