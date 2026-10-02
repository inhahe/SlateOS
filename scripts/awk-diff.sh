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
DIFF_PROG='awk'
# Declared because every invocation below is bounded with it; see `run_side`.
# Without this the harness would run on a host lacking `timeout` and silently
# lose its only protection against a subject that does not terminate.
DIFF_NEED=timeout
# `command -v awk` is not good enough: on a Debian-family system `/usr/bin/awk`
# is whatever `update-alternatives` last pointed at, and mawk is a legitimate
# answer. mawk is not the reference — it has no `--posix`, no `ENVIRON`
# ordering guarantees and a different `printf` — so gawk is named outright.
DIFF_REF='/usr/bin/gawk /usr/local/bin/gawk /usr/bin/awk'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

GNUFLAGS=${GNUFLAGS:---posix}

# The self-check (`OURS=/usr/bin/gawk`, see the header) runs the reference on
# both sides, and it must then be the same gawk on both: without `--posix` on
# our side too, every row about a `--posix` behaviour -- `\y` a literal, a
# backslash-newline in a string fatal, a directory operand an error -- would
# differ for a reason that has nothing to do with the harness. It is told by
# the two sides' `awk` being one file; `OURS` cannot tell it, because
# `diff-wsl.sh` sets `OURS` to the built binary on every ordinary run.
selfcheck=
[ "$(readlink -f "$bindir/ours/awk")" = "$(readlink -f "$bindir/gnu/awk")" ] && selfcheck=yes

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
# One line per thing a backslash can be made to mean: a dot, a backslash, a
# `t`, a tab, `a<TAB>b` and `atb`, `A` and `x41`, `]` and `\]`, a literal brace,
# a slash, `y`, `w`, `aa`, `a` followed by byte 1, and a bar-separated line.
printf '.\n\\\nt\n\tx\na\tb\natb\nA\nx41\n]\n\\]\na{b}c\n/\ny\nw\naa\na\001\nb|c|d\n' > esc.txt
printf 'a:b;c\n'                                            > semi.txt

# Program files, for -f.
printf '{ print "P1:" $0 }\n'                               > p1.awk
printf 'END { print "P2:" NR }\n'                           > p2.awk
printf '#!/usr/bin/awk -f\nBEGIN { print "shebang" }\n'      > p3.awk
# A runtime fatal on line 2, so a diagnostic has a file and a line to name.
printf 'BEGIN { x = 1 }\nBEGIN { print 1/z }\n'              > div.awk
# A backslash-newline inside a string on line 2: fatal under --posix.
printf 'BEGIN { x = 1 }\nBEGIN { print "a\\\nb" }\n'          > bsnl.awk

# One invocation of one side. `$1` is `ours` or `gnu`; `$2` names a fixture to
# feed on stdin, or `-` for none.
#
# The side's own directory comes first so that `awk` is the binary under test,
# but the real directories follow it: awk can run a shell (`system()`,
# `print | "cmd"`, `"cmd" | getline`), and with a one-entry PATH every such
# case degenerates into two shells agreeing that `cat` does not exist — which
# is a comparison of the harness against itself.
# Every invocation is bounded, on BOTH sides, the way `tsort-diff.sh` and 30
# other harnesses here already do it.
#
# The reason is specific and was paid for. `awk` is a programming language, so a
# subject that never terminates is not an exotic input, it is a normal one --
# and the standalone `awk` hung on `getline`, which is about as ordinary as awk
# gets. Unbounded, that stopped the whole run: two attempts left `awk` processes
# alive inside WSL for 35 and 25 minutes, and killing the hung *case* only let
# the harness advance to the next `getline` and hang again.
#
# The reference is wrapped too, and that is not symmetry for its own sake --
# `tsort-diff.sh` puts it best: "a harness that only bounded our side would hang
# on the day the reference was the buggy one."
#
# 30 seconds, `-k 2`: every case here is a few lines of input, so a subject
# still running after half a minute is not slow, it is stuck. `diff_run` keeps
# bash's own announcement of a signalled child out of the captured stderr.
run_side() {
  local side=$1 stdin=$2 out=$3 err=$4; shift 4
  local flags=
  [ "$side" = gnu ] && flags=$GNUFLAGS
  [ "$side" = ours ] && [ -n "$selfcheck" ] && flags=$GNUFLAGS
  if [ "$stdin" = "-" ]; then
    # shellcheck disable=SC2086
    diff_run timeout -k 2 30 env PATH="$bindir/$side:/usr/bin:/bin" awk $flags "$@" \
      </dev/null >"$out" 2>"$err"
  else
    # shellcheck disable=SC2086
    diff_run timeout -k 2 30 env PATH="$bindir/$side:/usr/bin:/bin" awk $flags "$@" \
      <"$stdin" >"$out" 2>"$err"
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

# `fsh_case PROGRAM WORDS` — PROGRAM run by `sh -c`, with WORDS after it as a
# shell reads them (operands, redirections), for the cases that are about
# standard output itself: `> /dev/full`. Where standard output went is the
# words' business; the diagnostics are compared as text, and the status.
fsh_case() {
  local prog=$1 words=$2 side err flags rc o_rc g_rc o_msg g_msg o_err g_err
  o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    flags=
    [ "$side" = gnu ] && flags=$GNUFLAGS
    [ "$side" = ours ] && [ -n "$selfcheck" ] && flags=$GNUFLAGS
    err=$o_err; [ "$side" = gnu ] && err=$g_err
    # The program is `$0` to the shell, so it needs no quoting of its own;
    # standard output defaults to nowhere, for words that do not say.
    # shellcheck disable=SC2086
    diff_run timeout -k 2 30 env PATH="$bindir/$side:/usr/bin:/bin" \
      sh -c "awk $flags \"\$0\" $words" "$prog" </dev/null >/dev/null 2>"$err"
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"
  AGREED_MSG=no
  [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ] && AGREED_MSG=yes
  REPORT=$(printf '  ours (rc=%s): {%s}\n  gnu  (rc=%s): {%s}' \
    "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  report "$AGREED_MSG" "sh -c 'awk $prog $words'"
}

# `tty_case PROGRAM` — PROGRAM with standard output and standard error on one
# pseudo-terminal (`script(1)`), the bytes the terminal received compared as a
# dump. On a terminal gawk flushes after every print (`output_is_tty`), so
# output and diagnostics interleave as they happen; a run that buffered
# standard output would show the stderr line first.
tty_case() {
  local prog=$1 side flags out o_out g_out
  for side in ours gnu; do
    flags=
    [ "$side" = gnu ] && flags=$GNUFLAGS
    [ "$side" = ours ] && [ -n "$selfcheck" ] && flags=$GNUFLAGS
    # The program goes through the environment, so it needs no quoting.
    # shellcheck disable=SC2016
    out=$(diff_run timeout -k 2 30 env PATH="$bindir/$side:/usr/bin:/bin" AWKPROG="$prog" \
      script -qec "awk $flags \"\$AWKPROG\"" /dev/null </dev/null | od -An -c)
    if [ "$side" = ours ]; then o_out=$out; else g_out=$out; fi
  done
  AGREED=no
  [ "$o_out" = "$g_out" ] && AGREED=yes
  REPORT=$(printf '  ours: %s\n  gnu:  %s' "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" \
    "$(printf '%s' "$g_out" | tr -s ' \n' ' ')")
  report "$AGREED" "tty: awk $prog"
}

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
# `\1` is the octal escape \001 in an awk regex, as POSIX's awk table says and
# gawk does. It was an xfail until 2026-10-01, when this awk read it as a
# backreference; see the escape section below.
run_case mixed '/(.)\1/'
run_case abc 'END {print NR}'
run_case abc 'BEGIN {print "start"} {print} END {print "stop"}'
run_case empty 'BEGIN {print "b"} END {print NR}'

# --- backslashes: awk's two escape layers ------------------------------------
# POSIX gives awk's regexes C's escapes and octal, "recognized both inside and
# outside bracket expressions"; gawk resolves them before regcomp (re.c
# make_regexp) and compiles under RE_SYNTAX_POSIX_AWK, where a backslash in a
# bracket quotes the next character and the GNU operators are plain letters.
# A string resolves its escapes by node.c parse_escape, and an escape it does
# not know is the character itself, with a warning. `ere::awk` is both layers.
# Warnings are compared by presence: gawk prefixes `cmd. line:1:` and ours has
# no source locations (the known gap, below) -- except for `-v`, `-F` and
# `var=value`, where gawk names no location either and the text is compared.
run_case esc '/^[\.]$/'
run_case esc '/^[\t]/'
run_case esc '/^a\tb$/'
run_case esc '/^\101$/'
run_case esc '/^\x41$/'
run_case esc '/^[\]]$/'
run_case esc '/^[\\]$/'
run_case esc '/^[\/]$/'
run_case esc '/^[^\]]$/'
run_case esc '/a{b}c/'
run_case esc '/a{1/'
run_case esc '/^\y$/'
run_case esc '/^\w$/ {print "w:" $0} /^\w$/ {print "again"}'
run_case esc '/^\8$/'
run_case esc '/(.)\1/'
run_case esc '/[[:alpha:]/]/'
run_case esc '/[]/]/'
run_case esc '/a\
x/'
run_case esc '/{2}a/'
run_case esc '/^{b}/'
# A repetition straight after an assertion is refused in RE_SYNTAX_POSIX_AWK
# as in every POSIX syntax; a group around one is an atom.
run_case esc '/a$*/'
run_case esc '/^*a/'
run_case esc '/a($)*/'
run_case esc '{ if ($0 ~ "^\\.$") print "dot:" $0 }'
run_case esc '{ if ($0 ~ "^a\.b$") print "any:" $0 }'
run_case esc '{ if ($0 ~ "^\\t") print "tab:" $0 }'
run_case esc '{ if ($0 ~ "^[\\.]$") print "bracket:" $0 }'
run_case esc 'BEGIN { s = "\q\/\8"; print s, length(s) }'
# No `length` here: `\777` is byte 0xFF, and gawk's warning about measuring
# an undecodable byte is a divergence recorded below, not this section's.
run_case esc 'BEGIN { s = "\101\x41\777"; print s }'
run_case esc '{ n = split($0, p, "\\|"); print n }'
run_case esc 'BEGIN { FS = "\\|" } { print NF }'
run_case esc 'BEGIN { FS = "a(" } { print $1 }'
run_case esc '{ if ($0 ~ "a(") print }'
run_case esc 'BEGIN { s = "a\
b" }'
msg_case esc -v 'v=a\qb' 'BEGIN { print v }'
msg_case esc -F '\|' 'NR == 17 { print NF }'
msg_case esc 'NR == 1 { print v }' 'v=x\qy'
# -F and -v are one list of assignments, applied in the order given.
run_case semi -F: -v 'FS=;' '{ print $1 }'
run_case semi -v 'FS=;' -F: '{ print $1 }'

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
# The parenthesised list, POSIX's `Print '(' multiple_expr_list ')'`. It was
# a syntax error here until 2026-10-01 -- `printf("%s\n", x)`, the form most
# programs are written in, refused before running -- and no case used it.
# What follows the `)` tells the list from an expression that begins with a
# grouping: the statement's end or a redirection, or more expression.
run_case table '{printf("%s-%s\n", $1, $2)}'
run_case table '{print($1, $2)}'
run_case table '{printf("%s|%s\n",
  $2, $1)}'
run_case abc '{print ($0 > "b")}'
run_case abc '{print ("<")($0)(">")}'
run_case abc '{print (NR), ($0)}'
run_case abc 'BEGIN {a[1,2]; print (1,2) in a, (2,1) in a}'
run_case abc '{print ("x", $0) ("y")}'
run_case abc 'BEGIN {print ()}'
wfile_case 'a parenthesised list, redirected' 'BEGIN {print("a", "b") > "out.txt"; printf("%d\n", 7) >> "out.txt"}'

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
# The *stderr* half is `cmd. line:1:` on this one and nothing on the
# first-operand case above. That is gawk's model of where it is: the line of
# the last thing that ran (`interpret.h` sets `sourceline` from every
# instruction it executes), so a file that fails after a rule has run is
# placed on that rule, and one that fails before anything has run is not
# placed at all. This was an xfail until 2026-10-01, recorded as "stale
# interpreter state" this harness would not fit itself to; but it is the same
# state that places every runtime diagnostic, and ours now keeps it the same
# way, so the two answers come out of one model rather than a special case.
fmsg_case '{print}' abc.txt /definitely/not/here

# --- where a diagnostic says it happened -------------------------------------
# gawk's `err()` writes `cmd. line:N:` (or `prog.awk:N:` for a `-f` file) when
# its current line is set, then `(FILENAME=f FNR=n) ` once a record has been
# read (`FNR` truncated to an integer and above 0). Its current line is that
# of the last instruction executed, which is why a fatal before anything ran
# is not placed, and why the file named is the one the line is in.
#
# `1/z`, not `1/0`, throughout: gawk folds a constant divisor at parse time,
# which is a different diagnostic (below).
fmsg_case 'BEGIN { x = 1 }
BEGIN { y = 1/z }'
fmsg_case '{ n++ }
END { print 1/z }' abc.txt
msg_case abc '{ n++ }
END { print 1/z }'
fmsg_case 'function f(x) {
  return x
}
BEGIN { y = f(1)
  print 1/z }'
fmsg_case -f div.awk
fmsg_case -f p1.awk -f div.awk
fmsg_case '
NR == 2 && 1/z { print }' abc.txt
fmsg_case '{ x = 1 }
1/z' abc.txt
fmsg_case '{ x = 1 }

END { print 1/z }' abc.txt
fmsg_case '
1/z, 0 { print }' abc.txt
fmsg_case 'END { print 1/z }' abc.txt
fmsg_case 'END { while ((getline line) > 0) n++
 print 1/z }' abc.txt
fmsg_case 'END { exit 1/z }' abc.txt
fmsg_case '{ print 1/$2 }' abc.txt
# `FNR` in the prefix is gawk's C `long`, truncated from whatever the program
# assigned, and the part is left out unless it is above 0. `FILENAME` is
# whatever the program made it.
fmsg_case '{ FNR = 0.5; print 1/z }' abc.txt
fmsg_case '{ FNR = -3; print 1/z }' abc.txt
fmsg_case '{ FILENAME = "zz"; print 1/z }' abc.txt
# Reading a file with `getline < f` moves neither `FNR` nor `FILENAME`; plain
# `getline` in BEGIN moves both.
fmsg_case 'BEGIN { getline line < "abc.txt"; print 1/z }'
fmsg_case 'BEGIN { getline; print 1/z }' abc.txt
fmsg_case 'BEGIN { getline x < "/definitely/not/here"
 print 1/z }'
# Statements in every position a statement can be in.
fmsg_case 'BEGIN { if (1)
 print 1/z }'
fmsg_case 'BEGIN { if (0) x = 1
 else
 y = 1/z }'
fmsg_case 'BEGIN {
 while (1/z) print }'
fmsg_case 'BEGIN { a = 1
 b = 2; c = 3; d = 1/z }'
fmsg_case 'BEGIN { if (1) {
 a = 1 } d = 1/z }'
fmsg_case 'BEGIN { a[1/z] = 1 }'
fmsg_case 'BEGIN { y = "a" (1/z) }'
fmsg_case 'BEGIN { if (z == 0 && 1/z) print }'
fmsg_case 'BEGIN { getline a[1/z] < "abc.txt" }'
fmsg_case 'BEGIN { (1/z) | getline }'
# The regex a program computes: its escape warnings and its refusal are placed
# too, with the record being read.
fmsg_case 'BEGIN { x = 1 }
{ if ($0 ~ "\\q") print }' abc.txt
fmsg_case 'BEGIN { x = 1
 if ("q" ~ "\\q") print "m" }'
fmsg_case 'BEGIN { s = "\\q"
 if ("q" ~ s) print "m" }'
fmsg_case 'BEGIN { x = 1
 print match("q", "\\q") }'
fmsg_case 'BEGIN { x = 1 }
{ if ($0 ~ "(") print }' abc.txt
fmsg_case '
$0 ~ "("' abc.txt
fmsg_case 'BEGIN { x = 1
 print match("a", "(") }'
fmsg_case 'BEGIN { x = 1; s = "a"
 sub("(", "x", s) }'
fmsg_case 'BEGIN { x = 1; s = "a"
 gsub("(", "x", s) }'
fmsg_case '{ x = 1
 FS = "((" }' abc.txt
# A separator of one character is that character, not a regex, so these are
# not errors at all.
fmsg_case 'BEGIN { x = 1
 print split("a", arr, "(") }'
fmsg_case 'BEGIN { x = 1
 FS = "(" }
{ print $1 }' abc.txt
# What the parser says, it says where it read it.
fmsg_case 'BEGIN { x = 1 }
BEGIN { print "\q" }'
fmsg_case 'BEGIN { x = 1 }
/\q/' abc.txt
fmsg_case 'BEGIN { x = 1 }
/a(/'
# A backslash-newline in a string is fatal under `--posix`, and gawk says so
# before it counts the newline: the line named is the backslash's.
fmsg_case -f bsnl.awk
fmsg_case 'BEGIN { x = 1 }
BEGIN { print "a\
b" }'
# Where nothing has run yet, nothing is named; after BEGIN, BEGIN's line is;
# after `nextfile`, the `nextfile`'s.
fmsg_case 'BEGIN{x=1}

{print}' /definitely/not/here
fmsg_case '
x == 1 { print }

{print}' /definitely/not/here
fmsg_case '{print}


END { print "end" }' /definitely/not/here
fmsg_case 'BEGIN { x = 1 }
END { print NR }' /definitely/not/here
fmsg_case '{ nextfile }' abc.txt /definitely/not/here

# --- what the tree walk got wrong --------------------------------------------
# Until 2026-10-01 the interpreter walked the parsed tree; it runs compiled
# instructions now (compile.rs, interp/run.rs). Every row here differed then.
#
# Recursion: the walk recursed natively once per awk call, so 3000 calls deep
# was a stack overflow, exit 134. gawk's frames are on the heap; ours are too.
fmsg_case 'function f(n) { return n ? f(n-1) : 7 } BEGIN { print f(3000) }'
fmsg_case 'function f(n) { return n ? f(n-1) : 7 } BEGIN { print f(30000) }'
# `exit`, `next` and `nextfile` from inside a function. The walk could not
# carry them out through the expression that made the call: `exit` printed an
# empty `awk: ` line and exited 2, `next` was ignored.
fmsg_case 'function f() { exit 3 } BEGIN { f(); print "no" }'
fmsg_case 'function f() { exit 4 } { f() } END { print "end" }' abc.txt
fmsg_case 'function f() { exit 5 } END { f(); print "no" }' abc.txt
fmsg_case 'function f() { exit 6 } f() { print "no" } END { print "end" }' abc.txt
fmsg_case 'function f() { next } { f(); print "no" } END { print NR }' abc.txt
fmsg_case 'function f() { next } f() { print "no" } { print "yes" }' abc.txt
fmsg_case 'function f() { nextfile } { print; f() } END { print NR }' abc.txt abc.txt
fmsg_case 'function f() { next } BEGIN { f() }'
fmsg_case 'function f() { next } END { f() }' abc.txt
fmsg_case 'function f() { nextfile } BEGIN { f() }'
# A read-modify-write evaluates its target once: the walk read `a[i++]` and
# then wrote a second `a[i++]`.
fmsg_case 'BEGIN { i = 1; a[i++] += 5; for (k in a) print k, a[k]; print "i=" i }'
fmsg_case 'BEGIN { i = 1; a[i++]++; for (k in a) print k, a[k]; print "i=" i }'
fmsg_case 'BEGIN { i = 1; ++a[i++]; for (k in a) print k, a[k]; print "i=" i }'
msg_case abc '{ i = 1; $(i++) += 0; print; print "i=" i }'
fmsg_case 'BEGIN { i = 1; a[1] = "xx"; sub(/x/, "y", a[i++]); for (k in a) print k, a[k]; print "i=" i }'
# gawk's order of evaluation: a print's redirection before its arguments, an
# assignment's value before its target, a `getline` target before the read.
wfile_case 'a print evaluates its redirection first' 'BEGIN { i = 1; print i++ > ("out" i ".txt") }'
fmsg_case 'BEGIN { i = 1; a[i++] = i; for (k in a) print k, a[k] }'
fmsg_case 'BEGIN { i = 1; r = (getline a[i++] < "/definitely/not/here"); print r, i }'
# A statement that spans lines is placed on the line of the operation that
# failed, the instruction's line, as gawk places it.
fmsg_case 'BEGIN {
  if (1 &&
      1/z) print }'
fmsg_case 'BEGIN { print 1,
 1/z }'
fmsg_case 'NR==1,
1/z { print }' abc.txt
fmsg_case 'BEGIN { x = 1
 if (x &&
     "q" ~ "\\q") print "m" }'
fmsg_case 'BEGIN { for (i = 0;
  i < 1/z; i++) print i }'
fmsg_case 'BEGIN { printf "%d %d\n", 1,
 1/z }'
fmsg_case 'BEGIN { do { x++ }
 while (1/z) }'
# Wordings the walk had its own way of saying.
fmsg_case 'BEGIN { x = 4
 x /= z }'
fmsg_case 'BEGIN { x = 4
 x %= z }'
fmsg_case 'BEGIN { x = 4; x /= 0 }'
fmsg_case 'BEGIN { x = 1 }
{ print $(-1) }' abc.txt
fmsg_case '{ print $(-0.5) }' abc.txt
fmsg_case '{ x = 1
 NF = -1 }' abc.txt
# More arguments than a function declares: gawk warns on every such call --
# placed at the call, after what was printed before it -- and drops the
# extras, which it has evaluated.
fmsg_case 'function f(a) { return a }
BEGIN { print f(1, 2); print f(3, 4, 5) }'
fmsg_case 'function f(a) { return a }
BEGIN { x = f(1,
 2) ; print f(1/z) }'
fmsg_case 'function f(a) { return a }
BEGIN { print f(1, i++); print i }'
fmsg_case 'function f(a) { return a }
BEGIN { b[1] = 1; print f(1, b) }'
# Plain `getline` that reaches an operand that will not open is fatal, as the
# main loop is; it was -1.
fmsg_case 'NR == 3 { r = getline line; print "r=" r }' abc.txt /definitely/not/here
fmsg_case 'BEGIN { r = getline; print "r=" r }' /definitely/not/here
# A directory operand is refused as it is opened, not when it is read.
fmsg_case '{ print }' /
fmsg_case '{ print }' abc.txt /
# `exit`'s status is C's conversion of the value, so 1e30 is 0.
fmsg_case 'BEGIN { exit 1e30 }'
fmsg_case 'BEGIN { exit -1 }'

# --- output, and how it fails ------------------------------------------------
# gawk's model, measured: a redirection that will not open is fatal at the
# statement (`cannot redirect to`); a write that fails is fatal there too,
# naming the statement (`print to "f" failed`) -- for a small write that is
# when the buffer is flushed, at `close`, `fflush` or exit. At exit gawk
# closes every redirection first, a failure there fatal and placed nowhere but
# the input, then flushes standard output, where a failure is only a warning
# and status 1 unless the program said `exit`. After a fatal error nothing
# more is said. All of it differed until 2026-10-01: a full disk under a
# redirection was silent, exit 0.
fmsg_case 'BEGIN { x = 1
 print "x" > "/nonexistent/dir/f" }'
fmsg_case 'BEGIN { print "x" >> "/nonexistent/dir/f" }'
fmsg_case 'BEGIN { printf "x" > "/nonexistent/dir/f" }'
fmsg_case 'BEGIN { print "x" > "/" }'
fmsg_case 'BEGIN { x = 1
 print "x" > "" }'
fmsg_case 'BEGIN { printf "x" >> "" }'
fmsg_case 'BEGIN { print "x" | "" }'
fmsg_case 'BEGIN { x = 1
 r = (getline line < ""); print r }'
fmsg_case 'BEGIN { x = 1
 r = ("" | getline line); print r }'
fmsg_case 'BEGIN { r = (getline l < "/"); print "r=" r }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; print "after" }'
fmsg_case 'BEGIN { printf "x" > "/dev/full"; print "after" }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; print "y" >> "/dev/full" }'
fmsg_case '{ print > "/dev/full" }' abc.txt
fmsg_case 'BEGIN { print "x" > "/dev/full"; r = close("/dev/full"); print "r=" r }'
fmsg_case 'BEGIN { s = sprintf("%9000s", "x"); x = 1
 print s > "/dev/full"; print "after" }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; print 1/z }'
fmsg_case 'BEGIN { for (i = 0; i < 100000; i++) print i | "head -1"; print "after" }'
fmsg_case 'BEGIN { for (i = 0; i < 3000; i++) print i | "true"; r = close("true"); print "r=" r }'
# Standard output itself, on a full device.
fsh_case 'BEGIN { print "x" }' '> /dev/full'
fsh_case 'BEGIN { print "x"; exit 0 }' '> /dev/full'
fsh_case 'BEGIN { print "x"; exit 3 }' '> /dev/full'
fsh_case '{ print }' 'abc.txt > /dev/full'
fsh_case 'END { print "x"; exit 0 }' 'abc.txt > /dev/full'
fsh_case 'BEGIN { s = sprintf("%9000s", "x"); x = 1
 print s; print "after" > "/dev/stderr" }' '> /dev/full'
fsh_case 'BEGIN { print "x"; r = fflush(); print "r=" r > "/dev/stderr" }' '> /dev/full'
fsh_case 'BEGIN { print "x"; print 1/z }' '> /dev/full'
# Standard output on a terminal is flushed after every print, as gawk's is.
# It was not until 2026-10-01: an interactive `awk '{ print $1 }'` held its
# answers until 8 KiB had built up or input ended.
if command -v script >/dev/null 2>&1; then
  tty_case 'BEGIN { print "a"; printf "b" > "/dev/stderr"; print "c" }'
  tty_case 'BEGIN { printf "a\n"; printf "b" > "/dev/stderr"; printf "c\n" }'
fi
# `close`, `fflush` and `system`, as `--posix` answers them: `close` is 0 once
# it succeeds (not the command's status), `system` is the full wait status,
# `fflush` warns about what it cannot flush.
fmsg_case 'BEGIN { print "x" | "cat >/dev/null; exit 3"; r = close("cat >/dev/null; exit 3"); print "r=" r }'
fmsg_case 'BEGIN { "echo hi; exit 3" | getline x; r = close("echo hi; exit 3"); print "r=" r, x }'
fmsg_case 'BEGIN { print "x" > "f.txt"; r = close("f.txt"); print "r=" r }'
fmsg_case 'BEGIN { getline l < "abc.txt"; r = close("abc.txt"); print "r=" r }'
fmsg_case 'BEGIN { print "x" > "f.txt"; close("f.txt"); r = close("f.txt"); print "r=" r }'
fmsg_case 'BEGIN { r = system("exit 3"); print "r=" r }'
fmsg_case 'BEGIN { r = system(""); print "r=" r }'
fmsg_case 'BEGIN { print "a"; system("echo b"); print "c" }'
fmsg_case 'BEGIN { print "a" > "f.txt"; system("cat f.txt"); print "c" }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; r = system("true"); print "r=" r }'
fmsg_case 'BEGIN { r = fflush("nope"); print "r=" r }'
fmsg_case 'BEGIN { getline l < "abc.txt"; r = fflush("abc.txt"); print "r=" r }'
fmsg_case 'BEGIN { "echo hi" | getline l; r = fflush("echo hi"); print "r=" r }'
fmsg_case 'BEGIN { print "x" > "f.txt"; r = fflush("f.txt"); print "r=" r }'
fmsg_case 'BEGIN { r = fflush(""); print "r=" r }'
fmsg_case 'BEGIN { print "x"; r = fflush("/dev/stdout"); print "r=" r }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; r = fflush("/dev/full"); print "r=" r }'
fmsg_case 'BEGIN { print "x" > "/dev/full"; r = fflush(); print "r=" r }'

# --- the command line's assignments ------------------------------------------
# gawk's `arg_assign`, for `-v` and for a `var=value` operand: refused if the
# name is a keyword or built-in, or the value holds a newline; said with no
# line and no input position, FNR being zeroed for the assignment -- and
# restored after it, which undoes an assignment *to* FNR.
fmsg_case '{ print $1 }' abc.txt 'FS=((' abc.txt
fmsg_case '{print}' abc.txt 'v=\q' abc.txt
fmsg_case 'END { print FNR }' abc.txt FNR=10
fmsg_case 'END { print NR }' abc.txt NR=10
fmsg_case '{ print FNR }' FNR=10 abc.txt
fmsg_case '{ print }' abc.txt v=1 /definitely/not/here
fmsg_case '{ a[1] = 1; print }' abc.txt a=1 abc.txt
fmsg_case 'function f() { } { print }' abc.txt f=1
fmsg_case '{ print NF }' abc.txt NF=-1 abc.txt
fmsg_case '{ print }' abc.txt if=1
fmsg_case '{ print }' abc.txt length=1
fmsg_case '{ print }' abc.txt ARGV=1
fmsg_case '{ print x }' 'x=a
b' abc.txt
fmsg_case 'END { print NR }' abc.txt NR=2.5
fmsg_case -v NF=-1 'BEGIN { print NF }'
fmsg_case -v FNR=2.5 'BEGIN { print FNR }'
fmsg_case -v 'FS=((' 'BEGIN { print "x" }'
fmsg_case -v a=1 'BEGIN { a[1] = 1 }'
fmsg_case -v f=1 'function f() { } BEGIN { print "x" }'
fmsg_case -v if=1 'BEGIN { print "x" }'
fmsg_case -v substr=1 'BEGIN { print "x" }'
fmsg_case -v ENVIRON=1 'BEGIN { print "x" }'
fmsg_case -v 'x=a
b' 'BEGIN { print x }'
fmsg_case -v '1x=1' 'BEGIN { print "x" }'
file_case -v x 'BEGIN { print "x" }'

# --- NR and FNR are C longs ----------------------------------------------------
# gawk keeps a `long` beside each: an assignment sets both, a record counts the
# long, and a read gives the long whenever the two disagree.
fmsg_case '{ NR = 2.5; print NR; NR = "7x"; print NR; exit }' abc.txt
fmsg_case 'NR == 1 { NR = 10.5 } { print NR, FNR }' abc.txt
fmsg_case '{ NR = -0; print NR; x = NR ""; print x; exit }' abc.txt
fmsg_case '{ NR = "10"; print (NR < 9); exit }' abc.txt
fmsg_case '{ FNR = 7.9 } END { print FNR, NR }' abc.txt
fmsg_case 'BEGIN { getline NR < "abc.txt"; print NR }'
fmsg_case 'BEGIN { NR = 2.5; print NR }'
# And in a diagnostic's prefix: FILENAME as a number has no text to show,
# and C's `%s` stops at a NUL.
fmsg_case '{ FILENAME = 5; print 1/z }' abc.txt
fmsg_case '{ FILENAME = "a" sprintf("%c", 0) "b"; print 1/z }' abc.txt
fmsg_case '{ FNR = 1e30; print 1/z }' abc.txt
fmsg_case '{ FNR = 2.9; print 1/z }' abc.txt

# --- substr, and the math functions' warnings -----------------------------------
# gawk 5.2.1 truncates substr's arguments; it does not round them.
fmsg_case 'BEGIN { print substr("hello", 1.5, 2.4) "|" substr("hello", 2.7) "|" substr("hello", 0.9, 2) "|" substr("hello", 2, 0.9) "|" substr("hello", 2, 1.9) "|" }'
fmsg_case 'BEGIN { print substr("hello", "x") "|" substr("hello", 2, "x") "|" substr("hello", 1e30) "|" substr("hello", -1e30, 1e30) "|" substr("hello", 0, 2) "|" substr("hello", -1, 3) "|" substr("", 1) "|" }'
fmsg_case 'BEGIN { n = log(-1); print substr("hello", n) "|" substr("hello", 2, n) "|" }'
fmsg_case 'BEGIN { print exp(-746) }'
fmsg_case 'BEGIN { print log(-1); x = -0; print log(x); print log(0); print sqrt(-4); print sqrt(x); print log(-0.5) }'
fmsg_case 'BEGIN { x = 1
 print log(-2.5e10) }'

# A multi-character RS is a regex here, as in gawk without --posix, mawk and
# the one true awk; gawk --posix takes its first character. See main.rs.
xfail_case 'a multi-character RS is a regex here; gawk --posix uses its first character' \
  abc 'BEGIN { RS = "ab" } { print NR ": " $0 }'

# --- printf, as gawk's format_tree --------------------------------------------
# Until 2026-10-01 `printf` was a hand-rolled C printf that refused what it did
# not know, laid floats out from `log10` (so `%.17g` lost its last digit and a
# subnormal printed as `infe-320`), had no `%a`, and saturated `%d` at a
# `long`. It is gawk's format_tree now, over Rust's exact float digits.
#
# What gawk cannot convert it copies as written; C's length modifiers are fatal
# under --posix.
for c in k y q b - . '*' '#' "'" ' ' + 0; do
  fmsg_case "BEGIN { printf \"[%$c]\\n\", 65 }"
done
for c in h l L j t z; do
  fmsg_case "BEGIN { x = 1
 printf \"[%${c}d]\\n\", 65 }"
done
fmsg_case 'BEGIN { printf "[%5" }'
fmsg_case 'BEGIN { printf "[%-5" }'
fmsg_case 'BEGIN { printf "abc%" }'
fmsg_case 'BEGIN { printf "[%5k]\n", 65 }'
fmsg_case 'BEGIN { printf "%k %d\n", 7 }'
fmsg_case 'BEGIN { x = sprintf("[%z]", 65); print x }'
fmsg_case 'BEGIN { printf "%1$d\n", 5 }'
# The flags are a state machine: read while the width is open.
fmsg_case 'BEGIN { printf "[%5-d] [%+ d] [% +d] [%*05d] [%-05d] [%.-3d] [%5.-3d] [%P5d]\n", 42, 42, 42, 8, 3, 4, 7, 8, 7 }'
fmsg_case 'BEGIN { printf "[%5%] [%-5%] [%%]\n" }'
# Integers: every digit of %d, two's complement for the unsigned ones, and %g
# for what does not fit.
fmsg_case 'BEGIN { printf "%d %d %d\n", 2^53, 2^64, -2^63 }'
fmsg_case 'BEGIN { x = 2^1024; printf "%d %d %i %x|\n", x, -x, log(-1), x }'
fmsg_case 'BEGIN { printf "%x %u %o %X\n", -1, -1, -1, 255 }'
fmsg_case 'BEGIN { printf "%x %.3x %#x\n", 2^70, 2^70, 2^64 - 2048 }'
fmsg_case 'BEGIN { printf "[%#o] [%#.0o] [%#x] [%.0x] [%#.0x] [%5.0d] [%.0d]\n", 0, 0, 0, 0, 0, 0, 0 }'
fmsg_case 'BEGIN { printf "[%#08x] [%#8x] [%-#8x] [%08.3d] [%.5d] [%+.5d] [%08d]\n", 255, 255, 255, 7, -42, 42, -42 }'
fmsg_case 'BEGIN { printf "%d %d %d %d\n", -0.5, 0.5, -1.5, 1e18 }'
# Floats, digit for digit with glibc.
fmsg_case 'BEGIN { printf "%.17g %.17g %.17g\n", 7^-21, 1.1^50, 0.1 }'
fmsg_case 'BEGIN { printf "%g %e %.3g %.3e %f\n", 1e-320, 1e-320, 5e-324, 5e-324, 1e-320 }'
fmsg_case 'BEGIN { printf "%.0f %.0f %.0f %.0f %.1f %.2f\n", 0.5, 1.5, 2.5, 3.5, 0.25, 0.125 }'
fmsg_case 'BEGIN { printf "[%#g] [%#.0f] [%#.0e] [%08.2f] [%-8.2f] [%+.3e] [% .3e]\n", 1, 1, 1, -1.5, 1.5, 12345, 12345 }'
fmsg_case 'BEGIN { x = 2^1024; printf "[%f] [%F] [%e] [%E] [%g] [%G] [%05f] [%-6f] [%+f]\n", x, -x, log(-1), -log(-1), x, -x, x, x, x }'
fmsg_case 'BEGIN { printf "%g %g %g %g %g %g\n", 100000, 1000000, 0.0001, 0.00001, 123456789, 1e100 }'
fmsg_case 'BEGIN { printf "%.10g %.20g %.0g %#.3g %G\n", 1/3, 2/3, 123, 1, 1e-10 }'
# %a, as glibc writes hexadecimal floating point.
fmsg_case 'BEGIN { printf "[%a] [%A] [%a] [%a] [%a] [%.1a] [%#a] [%a]\n", 65, 65, 1, 0, -0.5, 1 + 3/32, 1, 2^-1074 }'
fmsg_case 'BEGIN { printf "[%.0a] [%.0a] [%.2a] [%10a] [%-10a] [%010a] [%+a]\n", 1.5, 1.9, 1/3, 1, 1, 1, 1 }'
# %c: a string's first character -- the empty string's NUL, as gawk copies it
# -- and a number as glibc's wcrtomb writes it.
run_case abc 'BEGIN { printf "[%c] [%5c] [%-3c]\n", "", "", "abc" }'
run_case abc 'BEGIN { printf "[%c][%c][%c][%c]\n", -1, 55296, 57343, 1114111 }'
run_case abc 'BEGIN { printf "[%c][%c][%c][%c]\n", 1114112, 2147483647, 2147483648, 4294967361 }'

# --- numbers as strings, as gawk's format_val -----------------------------------
# An integral value prints whole within a `long` and through %.0f past it; the
# values that are not numbers are gawk's +inf, -inf, +nan, -nan.
fmsg_case 'BEGIN { print int(1e30), int(-1e30), 1e30, 2^63, 2^63 - 1024, 1e18 }'
fmsg_case 'BEGIN { x = 2^1024; print x, -x, log(-1), -log(-1) }'
fmsg_case 'BEGIN { x = 2^1024; y = x ""; print y; CONVFMT = "%d"; z = 0.5 ""; print z }'
fmsg_case 'BEGIN { x = 2^-1074; y = 1e-320; print y; printf "%g %e %.3g\n", y, y, y }'
fmsg_case 'BEGIN { OFMT = "%.2f"; print 3.14159, 17; x = 3.14159; print x "" }'
fmsg_case 'BEGIN { x = -0; print x, x "", -0 "" ; y = 0 * -1; print y }'
fmsg_case 'BEGIN { print exp(1000); print exp(-710); print exp(709) }'
fmsg_case '{ NR = 1e30; print NR; exit }' abc.txt
# `^` is gawk's, by repeated squaring.
fmsg_case 'BEGIN { x = 2; print x^1024, x^-1075, x^-1074, 0^0, (-0)^-1 }'
fmsg_case 'BEGIN { printf "%.17g %.17g\n", 3^33, 1.1^50 }'

# --- where a newline may stand ------------------------------------------------
# gawk --posix lets a newline follow `{ && || , ; do else`, the `)` of an `if`,
# `while`, `for` or a function's header, and a rule's `}`; nowhere else. Until
# 2026-10-01 ours also took one after `(`, `[`, `=`, `==`, `?` and `:`, before a
# `,` or a `)`, and between BEGIN and its `{` -- programs gawk refuses. Whether
# each is accepted, and the status, is compared.
run_case abc 'BEGIN
{ print "b" }'
run_case abc 'END
{ print "e" }'
run_case abc 'function f(a)
{ return 1 }
BEGIN { print f() }'
run_case abc 'function f(
a) { return 1 }
BEGIN { print f() }'
run_case abc 'function f(a
, b) { return 1 }
BEGIN { print f() }'
run_case abc 'function f(a,
b) { return 1 }
BEGIN { print f() }'
run_case abc 'function f(a
) { return 1 }
BEGIN { print f() }'
run_case abc 'BEGIN { if (
1) print "y" }'
run_case abc 'BEGIN { if (1
) print "y" }'
run_case abc 'BEGIN { for (i = 0
; i < 1; i++) print i }'
run_case abc 'BEGIN { for (i = 0; i < 1; i++
) print i }'
run_case abc 'function f(a) { return a }
BEGIN { print f(
1) }'
run_case abc 'function f(a, b) { return a b }
BEGIN { print f(1
, 2) }'
run_case abc 'function f(a) { return a }
BEGIN { print f(1
) }'
run_case abc 'BEGIN { print length(
"ab") }'
run_case abc 'BEGIN { print substr("abc",
2) }'
run_case abc 'BEGIN { print (
1) }'
run_case abc 'BEGIN { print (1
) }'
run_case abc 'BEGIN { a[1] = 2; print a[
1] }'
run_case abc 'BEGIN { a[1,2] = 2; print a[1,
2] }'
run_case abc 'BEGIN { a[1] = 2; print a[1
] }'
run_case abc 'BEGIN { a[1,2]; print ((
1,2) in a) }'
run_case abc 'BEGIN { printf(
"x\n") }'
run_case abc 'BEGIN { x =
1; print x }'
run_case abc 'BEGIN { x +=
1; print x }'
run_case abc 'BEGIN { print (1 ==
1) }'
run_case abc 'BEGIN { print (1 <
2) }'
run_case abc 'BEGIN { print (1 ?
2 : 3) }'
run_case abc 'BEGIN { print (1 ? 2 :
3) }'
run_case abc 'BEGIN { print (1 ? 2
: 3) }'
run_case abc 'BEGIN { a[1]; delete a[
1]; print length(a) }'
run_case abc 'BEGIN { print (1 && # c

1) }'
# One `;` after a rule's `}`, with newlines about it; not two, and not one
# before the first rule.
run_case abc ';BEGIN { print 1 }'
run_case abc 'BEGIN { print 1 };'
run_case abc 'BEGIN { print 1 }; END { print 2 }'
run_case abc 'BEGIN { print 1 };; END { print 2 }'
run_case abc 'BEGIN { print 1 };
;END { print 2 }'
run_case abc 'BEGIN { print 1 }
;
END { print 2 }'
run_case abc 'NR==1;NR==2'
run_case abc 'NR==1;;NR==2'
run_case abc ';'
run_case abc 'BEGIN { if (1) print 1;; else print 2 }'
run_case abc 'BEGIN { if (1) print 1

else print 2 }'
run_case abc 'BEGIN { if (1) { print 1 }; else print 2 }'
# `func` is not reserved by POSIX: a variable may have the name, and a function
# may not be defined with it (gawk --posix, which bwk's awk and gawk's own
# default mode do not follow).
run_case abc 'BEGIN { func = 3; print func }'
run_case abc '{ func = $1; print func }'
run_case abc 'func f() { return 1 }
BEGIN { print f() }'

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
# A divisor gawk folds to a zero constant is a parse-time `error:` at the
# operator, exit 1, and the parse carries on (`mk_binary`): the program never
# runs, and `BEGIN {if (0) print 1/0}` fails too. It was an xfail here until
# 2026-10-01, when ours was a runtime fatal. What gawk folds is narrow: a
# numeric literal, `-` or `!` of one, `^` of two -- never a string, `+0`, or
# anything in parentheses, which are runtime fatals instead.
msg_case abc 'BEGIN {print 1/0}'
fmsg_case 'BEGIN { if (0) print 1/0; print "ran" }'
fmsg_case 'function f() { return 1/0 }
BEGIN { print "x" }'
fmsg_case 'BEGIN { x = 1; print x/0.0 }'
fmsg_case 'BEGIN { print 1/-0 }'
fmsg_case 'BEGIN { x = 1; print x/1e-400 }'
fmsg_case 'BEGIN { print 2^-1/0 }'
fmsg_case 'BEGIN { x = 1; print x / 0^2 }'
fmsg_case 'BEGIN { x = 1; print x / !1 }'
fmsg_case 'BEGIN { x = 3; print x % 0 }'
fmsg_case 'BEGIN { print 1/(0) }'
fmsg_case 'BEGIN { print 1/(2-2) }'
fmsg_case 'BEGIN { print 1/+0 }'
fmsg_case 'BEGIN { print 1/"0" }'
fmsg_case 'BEGIN { x = 1; print x / 0^-1 }'
# Each `error:` is reported, in reading order; a syntax error or a regex
# literal that will not compile stops the parse after them, and a lexing
# error after them is reported after them (exit 2, being fatal).
fmsg_case 'BEGIN { print 1/0
 print 2%0 }'
fmsg_case 'BEGIN { print 1/0 }
/a(/'
fmsg_case 'BEGIN { print 1/0 }
BEGIN { print "a\
b" }'
fmsg_case '/a(/
/b(/'
fmsg_case 'BEGIN { print 1/0; nosuch() }'
# `next` and `nextfile` in BEGIN or END are refused as they are parsed.
fmsg_case 'BEGIN { next }'
fmsg_case 'END { next }' abc.txt
fmsg_case 'BEGIN { nextfile }'
fmsg_case 'END { x = 1
 nextfile }' abc.txt
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
# The stderr half was an xfail until 2026-10-01: gawk says `awk: cmd. line:1:
# fatal: …` and ours said `awk: fatal: …`, because nothing from `lex.rs`
# through `parse.rs` recorded where a statement came from. It does now (the
# section on placement, above, is the rest of that change).
run_case abc 'BEGIN {x = 0; print "before"; print 1/x}'
msg_case abc 'BEGIN {x = 0; print "before"; print 1/x}'
run_case abc 'BEGIN {x = 0; print 1 % x}'
msg_case abc 'BEGIN {x = 0; print 1 % x}'
xfail_case 'an array/scalar conflict is caught before the program runs (exit 1), not when first reached (gawk: exit 2)' abc 'BEGIN {x = 1; x[2] = 3}'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with gawk is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
