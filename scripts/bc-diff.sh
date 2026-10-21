#!/usr/bin/env bash
# Differential test: our bc against Ubuntu 24.04's, side by side in WSL.
#
# Ours is coreutils' `bc`: GNU bc 1.07.1 ported, its `main.c` in
# `userspace/coreutils/src/bin/bc.rs` and the rest in `userspace/gnubc`. It
# replaced a bc written from scratch, whose harness this file was.
#
# ## What is compared
#
# For each case: standard output byte for byte, standard error, the exit
# status. Each side runs with an empty environment but `PATH` (its own bc
# first, so `argv[0]` is the bare `bc` on both), `HOME`, and what the case
# sets. Standard input is not a terminal, so neither side is interactive
# unless `-i` makes it so.
#
# Most programs are run two ways, because bc reads each its own way: as a
# file operand (then standard input, empty), and on standard input -- where
# `read()` reads from the same descriptor through a buffer of its own.
#
# ## Deliberate differences
#
# `userspace/gnubc/src/lib.rs` lists them. The two this harness meets: a
# file name with an unprintable byte is escaped in a diagnostic, and there is
# no line editing, so `-i` on a pipe does not echo what it reads (xfail).
#
# `DIFF_FAIL_FAST=1` stops at the first difference; `VERBOSE=1` shows the
# agreements too.
set -u

DIFF_PROG='bc'
DIFF_NEED='python3 timeout'
DIFF_TIMEOUT=${DIFF_TIMEOUT:-3600}
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

cur=$DIFF_TMP/cur
work=$DIFF_TMP/work
pass=0; fail=0; xfail=0; xpass=0

# One side: a fresh working directory (which is also HOME), the case's
# setup, the command. ENVS is a list of assignments without spaces; ENVA is
# one assignment that may hold them.
run_side() {
  local side=$1 out=$2 err=$3; shift 3
  rm -rf "$work"
  mkdir -p "$work"
  ln -sfn "$bindir/$side" "$cur"
  (
    cd "$work" || exit 99
    if [ -n "${SETUP:-}" ]; then eval "$SETUP"; fi
    local input=/dev/null
    if [ -n "${INFILE:-}" ]; then input=$INFILE; fi
    if [ -n "${INPUT_SET:-}" ]; then
      printf '%s' "$INPUT" > "$DIFF_TMP/input"
      input=$DIFF_TMP/input
    fi
    # shellcheck disable=SC2086
    set -- env -i PATH="$cur:/usr/bin:/bin" HOME="$work" ${ENVS:-} ${ENVA:+"$ENVA"} \
      /usr/bin/timeout -k 2 "${LIMIT:-10}" "$@"
    case ${REDIR:-} in
      '>/dev/full')  diff_run "$@" <"$input" >/dev/full 2>"$err" ;;
      '>&-')         diff_run "$@" <"$input" >&- 2>"$err" ;;
      '2>/dev/full') diff_run "$@" <"$input" >"$out" 2>/dev/full ;;
      '<&-')         diff_run "$@" <&- >"$out" 2>"$err" ;;
      '<dir')        diff_run "$@" <"$work" >"$out" 2>"$err" ;;
      '|head')       { diff_run "$@" <"$input" 2>"$err"; echo $? > "$DIFF_TMP/pipe-rc"; } | head -c 20 >"$out"
                     exit "$(cat "$DIFF_TMP/pipe-rc")" ;;
      *)             diff_run "$@" <"$input" >"$out" 2>"$err" ;;
    esac
  )
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$DIFF_TMP/o.out; g_out=$DIFF_TMP/g.out; o_err=$DIFF_TMP/o.err; g_err=$DIFF_TMP/g.err
  : > "$o_out"; : > "$g_out"; : > "$o_err"; : > "$g_err"
  run_side ours "$o_out" "$o_err" "$@"; o_rc=$?
  run_side gnu "$g_out" "$g_err" "$@"; g_rc=$?
  if cmp -s "$o_out" "$g_out" && [ "$o_rc" = "$g_rc" ] && cmp -s "$o_err" "$g_err"; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(
    printf '  ours rc=%s out={%s} err={%s}\n' "$o_rc" "$(head -c 300 "$o_out" | od -An -c | tr -s ' \n' ' ' | head -c 400)" "$(head -c 300 "$o_err" | tr '\n' '|')"
    printf '  gnu  rc=%s out={%s} err={%s}\n' "$g_rc" "$(head -c 300 "$g_out" | od -An -c | tr -s ' \n' ' ' | head -c 400)" "$(head -c 300 "$g_err" | tr '\n' '|')"
    if ! cmp -s "$o_out" "$g_out"; then cmp "$o_out" "$g_out" 2>&1 | head -1 | sed 's/^/  /'; fi
    if ! cmp -s "$o_err" "$g_err"; then printf '  stderr differs\n'; fi
  )
  SETUP=''; INPUT=''; INPUT_SET=''; INFILE=''; ENVS=''; ENVA=''; REDIR=''; LIMIT=''
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$1" "$REPORT"
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then
      echo "bc-diff: stopped at the first difference"
      exit 1
    fi
  fi
  return 0
}

run_case() {
  local label
  label="$(printf '%q ' "$@")${REDIR:-} ${ENVS:-}${ENVA:+ $(printf '%q' "$ENVA")}${SETUP:+ [setup: $SETUP]}${INPUT_SET:+ [stdin: $(printf '%q' "$INPUT")]}"
  compare "$@"
  report "$label"
}

xfail_case() {
  local why=$1; shift
  local label
  label="$(printf '%q ' "$@")"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n%s\n' "$label" "$why" "$REPORT"
  fi
  return 0
}

# stdin PROGRAM ARGS...: bc ARGS with PROGRAM on standard input.
stdin() { local p=$1; shift; INPUT=$p; INPUT_SET=1; run_case bc "$@"; }
# file PROGRAM ARGS...: PROGRAM in a file, named as the last operand.
file() { local p=$1; shift; SETUP="printf '%s' $(printf '%q' "$p") > prog"; run_case bc "$@" prog; }
# two PROGRAM ARGS...: the program both ways, with each of the case's
# settings (they are cleared after each run).
two() {
  local e=${ENVS:-} a=${ENVA:-} l=${LIMIT:-}
  file "$@"
  ENVS=$e; ENVA=$a; LIMIT=$l; stdin "$@"
}
# lib PROGRAM: the program both ways under -l.
lib() { two "$1" -l; }
# raw FORMAT ARGS...: a program printf makes from FORMAT -- for bytes a shell
# string cannot hold, NUL among them -- as a file operand and on stdin.
raw() {
  local f=$1; shift
  SETUP="printf $(printf '%q' "$f") > prog"; run_case bc "$@" prog
  SETUP="printf $(printf '%q' "$f") > prog"; INFILE=prog; run_case bc "$@"
}

# --- the command line ------------------------------------------------------------
for args in '-h' '--help' '-v' '--version' '--he' '--ver' '-x' '--bogus' '--' '-hv' '-vh' '-q -v' \
            '-xq' '--warn=1' '--standard --version' '-c' '--compile' '-s' '-w' '-l' '--mathlib' '-ql'; do
  # shellcheck disable=SC2086
  run_case bc $args
done
run_case bc nonexistent
SETUP="printf '1\n'> a"; run_case bc a nonexistent
SETUP="printf '1\n'> a"; run_case bc nonexistent a
SETUP="printf '1\n' > a; printf '2\n' > b"; run_case bc a b
SETUP="printf '1\n' > a; printf '2\n' > b"; INPUT=$'3\n'; INPUT_SET=1; run_case bc a b
SETUP="printf '1\n' > a"; run_case bc a -q a
SETUP="printf 'quit\n' > a; printf '2\n' > b"; run_case bc a b
SETUP="printf 'halt\n' > a; printf '2\n' > b"; run_case bc a b
SETUP="printf '1+\n' > a; printf '2\n' > b"; run_case bc a b
SETUP="printf '1+' > a; printf '2\n' > b"; run_case bc a b
SETUP="printf 'define f() {\n' > a; printf '2\n}\nf()\n' > b"; run_case bc a b
SETUP="printf '/* open\n' > a; printf '*/ 5\n' > b"; run_case bc a b
SETUP="printf '\"open\n' > a; printf 'str\"\n7\n' > b"; run_case bc a b
SETUP="printf '1\\\\\n' > a; printf '2\n' > b"; run_case bc a b
SETUP="mkdir d"; run_case bc d
run_case bc /dev/null
INPUT=$'5\n'; INPUT_SET=1; run_case bc -
INPUT=$'5\n'; INPUT_SET=1; run_case bc /dev/stdin
REDIR='<dir'; run_case bc
REDIR='<&-'; run_case bc
SETUP="printf '6\n' > a"; REDIR='<&-'; run_case bc a
REDIR='>/dev/full'; INPUT=$'1\n'; INPUT_SET=1; run_case bc
REDIR='>/dev/full'; INPUT=$'print "x"\n'; INPUT_SET=1; run_case bc
REDIR='>/dev/full'; INPUT=$'warranty\n'; INPUT_SET=1; run_case bc
REDIR='>/dev/full'; INPUT=$'limits\n'; INPUT_SET=1; run_case bc
REDIR='>/dev/full'; run_case bc --version
REDIR='>/dev/full'; run_case bc --help
REDIR='>/dev/full'; INPUT=$'1\n'; INPUT_SET=1; run_case bc -c
REDIR='>&-'; INPUT=$'1\n'; INPUT_SET=1; run_case bc
REDIR='2>/dev/full'; INPUT=$'1/0\n'; INPUT_SET=1; run_case bc
REDIR='2>/dev/full'; INPUT=$'1+\n'; INPUT_SET=1; run_case bc
REDIR='2>/dev/full'; run_case bc -x
REDIR='|head'; INPUT=$'for(i=0;i<100;i++) i\n'; INPUT_SET=1; run_case bc
xfail_case 'a file name is escaped in a diagnostic' bc "$(printf 'no\nsuch')"

# -i: interactive whatever standard input is -- the banner, unless -q.
# Ubuntu's bc then reads standard input through readline, which echoes each
# line it reads when its input is not a terminal; ours has no line editing
# (lib.rs), and echoes nothing.
why_rl='readline echoes the lines it reads (no line editing here)'
INPUT=$'1\n'; INPUT_SET=1; xfail_case "$why_rl" bc -i
INPUT=$'1\n'; INPUT_SET=1; xfail_case "$why_rl" bc -iq
INPUT=$'1\nwarranty\n'; INPUT_SET=1; xfail_case "$why_rl" bc -i
SETUP="printf '2\n' > a"; INPUT=$'1\n'; INPUT_SET=1; xfail_case "$why_rl" bc -i a
# The banner and the rest, which readline leaves alone: from a file.
SETUP="printf '1\nwarranty\n' > a"; run_case bc -i a

# --- the environment -------------------------------------------------------------------
for e in 'BC_ENV_ARGS=-q' 'BC_ENV_ARGS=-l' 'BC_ENV_ARGS=-x' 'BC_ENV_ARGS=-v' 'BC_ENV_ARGS=-h' \
         'BC_ENV_ARGS=nonexistent' 'BC_ENV_ARGS=' 'BC_ENV_ARGS=--warn' 'BC_ENV_ARGS=--standard' \
         'POSIXLY_CORRECT=1' 'POSIXLY_CORRECT=' 'BC_LINE_LENGTH=0' 'BC_LINE_LENGTH=1' 'BC_LINE_LENGTH=2' \
         'BC_LINE_LENGTH=3' 'BC_LINE_LENGTH=4' 'BC_LINE_LENGTH=10' 'BC_LINE_LENGTH=-5' 'BC_LINE_LENGTH=abc' \
         'BC_LINE_LENGTH=12abc' 'BC_LINE_LENGTH=+20' 'BC_LINE_LENGTH=99999999999' 'BC_LINE_LENGTH=4294967306'; do
  ENVS=$e; two $'2^300\nprint "a long string a long string a long string a long string a long string a long string"\n1.5\n'
done
ENVA='BC_ENV_ARGS= -q  -l '; two $'s(1)\n'
ENVA='BC_ENV_ARGS=-w -s'; two $'a[1]=1\n'
SETUP="printf '5\n' > envf"; ENVA='BC_ENV_ARGS=envf'; two $'6\n'
SETUP="printf '5\n' > envf"; ENVA='BC_ENV_ARGS=envf'; INPUT=$'7\n'; INPUT_SET=1; run_case bc
ENVS='POSIXLY_CORRECT=1'; two $'a[1]=1\nx = 2 # c\nprint "s"\n'
ENVS='POSIXLY_CORRECT=1'; run_case bc -x a

# --- numbers ------------------------------------------------------------------------------
for p in '0' '00' '000.000' '1' '-1' '.5' '-.5' '5.' '1.50' '0.000' '12345678901234567890123456789012345678901234567890' \
         'A' 'F' 'G' 'Z' 'AB' 'A.B' '1A' 'ZZ.Z' '.A' '1.2.3' '1..2' '..' '.' '1,000' '1e5' \
         '123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890' \
         '-123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890' \
         '.000000000000000000000000000000000000000000000000000000000000000000000000000000001'; do
  two "$p"$'\n'
done
two $'1\\\n2\n'
two $'1\\\n\\\n2\n'
two $'1.\\\n5\n'
two $'1\\\n'
raw '1\\'
two $'1\\\r\n2\n'

# --- bases ----------------------------------------------------------------------------------
for p in 'ibase=16; FF' 'ibase=16; ff' 'ibase=2; 1010' 'ibase=2; 1012' 'ibase=2; 2' 'ibase=2; 1.1' \
         'ibase=16; 1.8' 'ibase=16; A.' 'ibase=16; .F' 'ibase=36; ZZ' 'ibase=37' 'ibase=1' 'ibase=0' 'ibase=-5' \
         'ibase=17' 'ibase=100' 'ibase=99999999999' 'ibase=16; ibase=A; ibase' 'ibase=8; 9' 'ibase=8; 19' \
         'obase=16; 255' 'obase=2; 10' 'obase=2; -10' 'obase=8; 0.5' 'obase=16; 1/3' 'obase=17; 1000' \
         'obase=100; 123456789' 'obase=1000; 2^100' 'obase=65536; 2^100' 'obase=1' 'obase=0' 'obase=-3' \
         'obase=99999999999' 'obase=2147483647; 5' 'obase=2147483648' 'obase=16; scale=10; 1/3' \
         'obase=3; scale=5; 1/7' 'obase=10; ibase=16; obase' 'scale=-1' 'scale=99999999999' \
         'scale=2147483647; scale' 'scale=5; 1/3; scale=0; 1/3' 'x=ibase; x' 'ibase=16; x=A; x' \
         'ibase++; ibase' 'ibase--; ibase' 'ibase=16; ibase++; ibase' 'ibase=2; ibase--' 'obase=2; obase--' \
         'scale--' 'scale=3; scale--; scale' 'obase=2147483647; obase++' 'history' 'history=5; history' \
         'history=-3; history' 'history=99999999999; history' 'history--; history' 'history=0; history--; history' \
         'history++; history' 'history=2147483648; history' 'last' '5; last' '5; .' '5; . + 1' '. = 3; .'; do
  two "$p"$'\n'
done

# --- arithmetic --------------------------------------------------------------------------------
for p in '1+2' '1-2' '2*3' '7/2' '7%2' '2^10' '2^-1' '2^-1; scale=5; 2^-1' '0^-1' '0^0' '(-2)^3' '-2^2' \
         '2^0.5' '2^99999999999' '10^100' '2^1000' '1/0' '1%0' '0/5' '-7/2' '-7%2' '7%-2' 'scale=3; 7%2' \
         'scale=3; 10%3' 'sqrt(2)' 'sqrt(-1)' 'sqrt(0)' 'sqrt(1)' 'scale=20; sqrt(2)' 'sqrt(1000000)' \
         'scale=5; sqrt(0.0001)' 'length(123)' 'length(0.001)' 'length(0)' 'length(-12.34)' 'length(.5)' \
         'scale(1.234)' 'scale(1)' 'scale(1.0)' 'x=5; ++x; x' 'x=5; x++; x' 'x=5; --x' 'x=5; x--; x' \
         'x+=3; x' 'x=2; x*=5; x' 'x=10; x/=4; x' 'x=10; x%=4; x' 'x=2; x^=10; x' 'x=5; x-=7; x' \
         'x =- 3' 'x=+3' 'x=-3; x' 'a=b=c=4; a; b; c' '1==1' '1!=1' '1<2' '1<=1' '1>2' '1>=2' \
         '2>1>0' '!0' '!5' '!!3' '1&&0' '1&&2' '0||0' '0||3' '1&&1||0' '!1||1' '(1+2)*3' '1+2*3' \
         '-(-3)' '- -3' '2^3^2' '2*-3' '1/3*3' 'scale=10; 1/3*3' 'scale=1; 0.05+0.05' \
         'x=3; x == 3' 'x = 3 == 3; x' '(x=3)' 'random()' 'random(); random(); random()' 'x=random(); x>0' \
         'read' 'length' 'sqrt' 'scale(1' '1 +' '(1' '1)' '*1' 'x++++' '++x++' '++5' '5++' '--' \
         '1;2;3' '1;;2' ';' ';;;' '{1;2}' '{}' '{ }' '{1}{2}' 'x=1;{x=2;x}' '{1' '}' '1}'; do
  two "$p"$'\n'
done

# --- strings and print -----------------------------------------------------------------------
for p in '"hello"' '"a\nb"' '"tab\there"' '"q\q"' '"bs\\"' '"x\"' '"\a\b\f\r\e\z"' '""' \
         '"multi
line"' 'print "a", 1, "b\n"' 'print 1' 'print 1, 2' 'print "x\n", 3+4, "\n"' 'print "\q\n"' \
         'print "unterminated' '"unterminated' 'print' 'print ,' 'print 1,' 'print a[1], "\n"' \
         'print "\' 'print "\\"' 'print "\t\n"' 'print "1234567890123456789012345678901234567890123456789012345678901234567890123456789\n"' \
         '"1234567890123456789012345678901234567890123456789012345678901234567890123456789"' \
         'print 2^300, "\n"'; do
  two "$p"$'\n'
done
raw '"a\000b"\n5\n'
raw 'print "a\000b"\n5\n'
raw '"\000"\n'

# --- names, variables, arrays ----------------------------------------------------------------
for p in 'x' 'x=5; x' 'abc=5; abc' 'a_1=2; a_1' 'a1b2=3; a1b2' '_a=1' 'A=1' 'x[0]' 'x[0]=5; x[0]' \
         'x[1]=2; x[0]=1; x[0]+x[1]' 'x[-1]' 'x[-1]=3' 'x[0.5]' 'x[1.5]=4; x[1]' 'x[16777215]=1; x[16777215]' \
         'x[16777216]' 'x[99999999999]' 'x[2^31]' 'x=1; x[0]=2; x; x[0]' 'x[0]=1; x[0]++; x[0]' \
         'x[0]=1; ++x[0]; x[0]' 'x[0]=5; x[0]--; --x[0]; x[0]' 'x[0]+=5; x[0]' 'x[1000]=7; x[1000]; x[999]' \
         'x[x[0]=3]=4; x[3]' 'x[]' 'x[' 'x[1' 'x[]=1' 'x[1]=x[2]=x[3]=9; x[1]+x[2]+x[3]' \
         'for(i=0;i<300;i++) a[i]=i; a[255]; a[256]; a[299]' 'a[x++]=x; a[0]; x' 'last=5; last' \
         'ibase=16; x[A]=1; x[10]' 'quit=1' 'define=1' 'if=2' 'Print=1'; do
  two "$p"$'\n'
done

# --- control flow --------------------------------------------------------------------------------
for p in 'if (1) 5' 'if (0) 5' 'if (0) 5 else 6' 'if (1) 5 else 6' $'if (1)\n5' $'if (0) 5\nelse 6' \
         'if (1) if (0) 1 else 2' 'if (x=3) x' 'if 1 5' 'if (1)' 'if () 5' 'else 5' \
         'i=0; while (i<3) i++' 'i=0; while (i<3) { i; i+=1 }' 'while (0) 5' 'while (1) break' \
         'i=0; while (1) { if (i++>3) break; i }' 'for (i=0; i<3; i++) i' 'for (i=0; i<3; i++) { i; continue; 9 }' \
         'for (;;) break' 'for (i=0;;i++) if (i>2) break' 'for (i=5; i; ) i--' 'for (; i<3; i++) i' \
         'for (i=0; i<2; ) { i; i++ }' 'for (i=0; i<3; i++) for (j=0; j<2; j++) i*10+j' 'break' 'continue' \
         'while (1) continue' 'for (i=0;i<3;i++) { while (1) break; i }' 'for (i=0; i<3; i++) ; i' \
         'while (i<3) i++; i' 'for (i=0; i<10; i++) if (i%2) continue else i' 'for (a[0]=0; a[0]<2; a[0]++) a[0]' \
         'for (i=0; i<2; i++) halt' 'if (0) halt; 5' 'if (0) quit; 5' 'quit; 5' '5; quit' 'halt; 5' '5; halt' \
         'for (i=0; i<2; i++) quit' 'while (i<3) { i++; if (i==2) halt }' 'for (i=0, i<3, i++) i' \
         'for i=0; i<3; i++ i' 'for (x(); i<3; i++) i' 'for ((i=0); (i<3); (i++)) i' 'while (1) { }' 'for (;;) { }'; do
  LIMIT=3 two "$p"$'\n'
done

# --- functions --------------------------------------------------------------------------------------
for p in $'define f(x) { return x*2 }\nf(3)' $'define f(x) { return (x*2) }\nf(3)' \
         $'define f() { return }\nf()' $'define f() { return () }\nf()' $'define f() { 5 }\nf()' \
         $'define f() { }\nf()' $'define f(x,y) { return x-y }\nf(5,2)' $'define f(x,y,z) { return x*100+y*10+z }\nf(1,2,3)' \
         $'define f(x) { auto y; y=x+1; return y }\nf(1); y' $'define f(x) { auto y,z; y=2; z=3; return x+y+z }\nf(1)' \
         $'define f(x) {\nauto y\ny = x\nreturn y\n}\nf(4)' $'define f(n) { if (n<2) return 1; return n*f(n-1) }\nf(20)' \
         $'define fib(n) { if (n<2) return n; return fib(n-1)+fib(n-2) }\nfib(15)' \
         $'define f(a[]) { return a[0]+a[1] }\nb[0]=1; b[1]=2; f(b[])' \
         $'define f(a[]) { a[0]=9; return 0 }\nb[0]=1; z=f(b[]); b[0]' \
         $'define f(*a[]) { a[0]=9; return 0 }\nb[0]=1; z=f(b[]); b[0]' \
         $'define f(a[]) { return a[5] }\nf(b[])' \
         $'define f(x) { return x }\nf(a[])' $'define f(x,y) { return x }\nf(a[],1)' \
         $'define f(x[]) { return 1 }\nf(1)' $'define f(x[]) { return 1 }\nq=1\nf(1)' $'define f(x) { return x }\nf(1,2)' \
         $'define f(x,y) { return x }\nf(1)' $'define f(x) { return x }\ny=2;z=3\nf(a[])' $'define f() { return 1 }\nf(1)' \
         $'g()' $'define f() { return g() }\nf()' $'define f(x) { x = 5 }\nx=1; f(2); x' $'define f(x) { auto x; return 1 }' \
         $'define f(x,x) { return 1 }' $'define f() { auto a,a; return 1 }' $'define f() { auto *a[]; return 1 }' \
         $'define f() { auto a[]; a[0]=1; return a[0] }\nf(); a[0]' $'define f(x) { return x }\ndefine f(x) { return x+1 }\nf(1)' \
         $'define void f() { print "v\\n" }\nf()' $'define void f() { return 5 }' $'define void f() { return }\nf()' \
         $'define void f() { }\nx=f()' $'define void f() { }\nf()+1' $'define void f() { }\n(f())' \
         $'define void f() { }\nprint f()' $'define f() { return 1 }\ndefine void f() { }\nf()' \
         $'define f()\n{ return 3 }\nf()' $'define f() {\n}\nf()' $'define f() { return 1 }' \
         $'define f() { return 1 } 5' $'define f() {\nreturn 1 }; 5' $'define f(x) { return x }\nf(1)+f(2)*f(3)' \
         $'define f(x) { ibase=16; return 10 }\nf(1); ibase; 10' $'define f(x) { return A }\nibase=16; f(0)' \
         $'define f(x) { return 10 }\nibase=16; f(0)' $'define f() { scale=5; return 1/3 }\nf(); scale' \
         $'define f() { return 1/0 }\nf(); 5' $'define f(x) { auto y; y=1/0 }\ny=7; f(1); y' \
         $'define f(x) { x; return x+1 }\nf(f(f(1)))' $'define f() { halt }\n1; f(); 2' $'define f() { quit }\n1; f(); 2' \
         $'define f(x) { return f(x) }' $'return 5' $'return' $'define f() { break }' \
         $'define f() { continue }' $'define f() { for(;;) return 3 }\nf()' $'define f() { while (1) { return 7 } }\nf()' \
         $'define a(x) { return 1 }\na(2)' $'define e(x) { return 1 }\ne(2)' $'define f(a[], b[]) { return a[0]*b[0] }\nx[0]=3; y[0]=4; f(x[], y[])' \
         $'define f(a[]) { return a[0] }\nf(a[])' $'define f(*a[]) { a[1]=5 }\nz=f(a[]); a[1]' $'define f(x[]) { return f2(x[]) }\ndefine f2(y[]) { return y[0] }\nz[0]=4; f(z[])'; do
  LIMIT=5 two "$p"$'\n'
done

# --- the math library -------------------------------------------------------------------------------
for p in 's(1)' 'c(1)' 'a(1)' 'l(2)' 'e(1)' 'j(0,1)' 'j(1,2.5)' 'scale' 's(0)' 'c(0)' 'a(0)' 'l(1)' 'e(0)' \
         'l(0)' 'l(-1)' 'e(-1)' 'e(100)' 's(-1)' 'a(-1)' 'a(.2)' 'a(5)' 'scale=50; 4*a(1)' 'scale=5; e(1)' \
         'scale=0; e(1)' 'scale=0; s(1)' 'scale=100; l(10)' 'j(-1,1)' 'j(1.5,1)' 'ibase=16; e(1)' 'obase=16; e(1)' \
         'e(1); scale' 'define e(x) { return 42 }; e(1)' 'define s(x) { return 7 }; c(1)' 'x=e(1); x'; do
  lib "$p"$'\n'
done
two $'s(1)\n'
INPUT=$'s(1)\n'; INPUT_SET=1; run_case bc -c -l

# --- read() -----------------------------------------------------------------------------------------
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'42\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'1.\n2\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'ibase=16\nx=read()\nx\n' > r"; INPUT=$'1.\n2\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'-12.5\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'   +7\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'1\\\n2\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'abc\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'ZZ\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'1+2\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\ny=read()\nx;y\n' > r"; INPUT=$'1 2\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'~5\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'.\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'ibase=2\nx=read()\nx\n' > r"; INPUT=$'19\n'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'5'; INPUT_SET=1; run_case bc r
SETUP="printf 'x=read()\nx\n' > r"; INPUT=$'-\n5\n'; INPUT_SET=1; run_case bc r
LIMIT=2 stdin $'x=read()\nx\n'
INPUT=$'x=read()\nx\n'; INPUT_SET=1; LIMIT=2 run_case bc -s

# --- comments and blanks -------------------------------------------------------------------------------
for p in '/* c */ 5' $'/* multi\nline */ 5' '/* unterminated' $'/* a */ /* b */ 6' '/*/ 7 */ 8' '/** 9 **/' \
         '# hash' '5 # hash' $'5 #\n6' '#' $'\t5' $'5\t+\t1' '  5  ' $'\f5' $'\v5' $'5\r' $'\r5'; do
  two "$p"$'\n'
done

# --- illegal characters and syntax errors ------------------------------------------------------------------
for p in '@' '$' '`' '~' '?' ':' "'" '\' '1 @ 2' '&' '|' '&&' '||' '1 & 2' '1 | 2' '= 1' '==' '<' '1 <' \
         '1 =< 2' '1 => 2' '1 <> 2' 'x =+ 1' 'x =* 2' 'x =/ 2' '{' '(' ')' '[' ']' ',' '1,2' \
         'define' 'define f' 'define f(' 'define f()' 'define f() {' 'define f() }' 'auto x' 'if' 'while' 'for' \
         'f(' 'f(1' 'f(,)' 'f(1,)' 'f(a[1])' 'x[1][2]' 'sqrt()' 'length()' 'scale()' 'read(1)' 'random(1)' \
         'limits' 'warranty' 'limits; 5' 'print limits' '1 2' '1 x' 'x y' '++' '-' '!' '1 !' 'ibase[1]' 'last[1]' \
         'scale(x[])' 'x[]+1' 'void' 'define void' 'Void=1'; do
  two "$p"$'\n'
done
raw '\001\n5\n'
raw '\177\n5\n'
raw '\200\n5\n'
raw '\377\n5\n'
raw '\000\n5\n'
raw '5\000\n6\n'
raw 'x\033y\n5\n'
raw '1+\n2\n'
raw '(\n5\n'
raw '1+\n\n\n2\n'
raw 'define f() {\n1+\n}\nf()\n5\n'
raw 'if (1) {\n1+\n5\n}\n6\n'
raw '1\n2'
raw '2+2'
raw 'print "A"'
raw 'define f() { return 1 }'
raw '5;'
raw '{ 5'
raw '/* c */'

# --- -s and -w: what is not POSIX bc -------------------------------------------------------------------------
for p in 'a[1]=1' 'abc=1' 'x=2; x+=1' 'print "a"' 'if (1) 1 else 2' 'for (;;) break' 'for (i=0; i<2; i++) continue' \
         '!0' '1&&1' '1||0' 'x=1 == 1' 'last' '.' 'history' 'read()' 'random()' 'halt' 'ibase=17' 'ibase=16; G' \
         'ibase=16; 1G' '"str"' '# c' $'define f(x) { return x }\nf(1)' $'define f(x) { return (x) }' \
         $'define f(*a[]) { return 1 }' $'define void f() { }' $'define f() {\nreturn 1 }' $'define f() { auto a[]; return 1 }' \
         'x=1, 2' '1 == 1' '(1 == 1)' 'if (1 == 1) 1' 'x = (1 < 2)' 'length(1 == 1)' 'sqrt(1 > 0)' 'a[1 < 2]=3' \
         'scale(1); length(2)' 'x=1;;' 'limits' 'warranty' 'quit'; do
  two "$p"$'\n' -s
  two "$p"$'\n' -w
done

# --- -c: the compiled code ------------------------------------------------------------------------------------
for p in '1+2' 'x=5' 'a[1]=2' 'print "a"' 'if (1) 2 else 3' 'while (0) 1' 'for (i=0;i<2;i++) i' \
         $'define f(x,*y[]) { auto z,w[]; return x }\nf(1,y[])' '"str"' 'ibase=16; FF' '1.5' 'halt' 'quit' '1+' \
         'for (i=0;i<100;i++) { x = x + i * 2 - 3 / 4 % 5 ^ 6 }' 'x++; ++x; x--; --x; a[1]++; ++a[1]' \
         'x+=1; a[2]*=3' '!x && y || z' 'x < y; x <= y; x > y; x >= y; x == y; x != y' 'read(); random()' \
         'sqrt(2); length(3); scale(4)' 'last; history; ibase; obase; scale'; do
  two "$p"$'\n' -c
done

# --- line breaking ---------------------------------------------------------------------------------------------------
for p in '2^500' '-2^500' '10^69' '10^68' '10^70' 'obase=16; 2^500' 'obase=1000; 2^500' 'scale=200; 1/7' \
         'print 2^300, 2^300, "\n"' 'print "x"; 2^300' '1; 2^200; print "abc"; 2^200'; do
  two "$p"$'\n'
  ENVS='BC_LINE_LENGTH=20'; two "$p"$'\n'
  ENVS='BC_LINE_LENGTH=0'; two "$p"$'\n'
done

# --- runtime errors and where they stop ---------------------------------------------------------------------------
for p in '1/0; 5' $'1/0\n5' '1%0' 'x=1/0; x' 'sqrt(-4); 5' 'a[-1]=1; 5' 'x=a[-1]' 'a[-1]++' '++a[-1]' \
         'a[16777216]--' $'define f(x) { return 1/0 }\nf(1); f(2)' 'for (i=0;i<3;i++) { i; 1/0 }' \
         $'1/0; print "after"\nprint "next\\n"' '2^(2^40)' '0^-1' '(1/0)+(2/0)' 'x[1/0]=1' 'sqrt(1/0)'; do
  two "$p"$'\n'
done

# --- seeded random programs ------------------------------------------------------------------------------------------
gen=$DIFF_TMP/gen.py
cat > "$gen" <<'PY'
import random, sys
r = random.Random(20261016)
atoms = ['+', '-', '*', '/', '%', '^', '=', '+=', '-=', '*=', '==', '!=', '<', '<=', '>', '>=', '!', '&&', '||',
         '++', '--', '(', ')', '(', ')', '[', ']', '{', '}', ';', ',', '\n', '\n', ' ', ' ', 'x', 'y', 'a[0]', 'a[',
         'f(', 'f(1)', 'g(x)', 'b[]', 'scale', 'ibase', 'obase', 'last', '.', 'length(', 'sqrt(', 'scale(',
         'if (', 'else', 'while (', 'for (', 'break', 'continue', 'return', 'auto', 'define', 'print', '"s"',
         '"', '/*', '*/', '#', '\\\n', '@', '\x01', '\xff', 'A', 'F', 'Z', 'void', 'read', 'limits']
for i in range(int(sys.argv[1])):
    parts = []
    for _ in range(r.randint(1, 30)):
        if r.random() < 0.35:
            parts.append(r.choice(['0', '1', '2', '3', '7', '10', '16', '99', '0.5', '1.25', '.5',
                                   str(r.randint(0, 10**r.randint(1, 30))), '-3']))
        else:
            parts.append(r.choice(atoms))
    text = ''.join(parts)
    if r.random() < 0.85:
        text += '\n'
    sys.stdout.buffer.write(text.encode('latin-1') + b'\0')
PY
while IFS= read -r -d '' prog; do
  LIMIT=3 two "$prog"
done < <(python3 "$gen" 400)

echo "bc-diff: $pass passed, $fail differed, $xfail xfail, $xpass xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
