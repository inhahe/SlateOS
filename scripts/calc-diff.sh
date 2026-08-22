#!/usr/bin/env bash
# Differential test: our bc and dc against GNU bc 1.07.1 and GNU dc 1.4.1.
#
# A calculator is the one utility where a unit test proves the least. Its
# arithmetic can be checked by hand, but *how it writes an answer down* is
# convention with no specification worth the name: POSIX says bc wraps a long
# number and does not say at what column, says nothing at all about dc's `a`,
# and says nothing about whether a value below one keeps its leading zero. Every
# script that captures `$(echo "scale=2;1/2" | bc)` depends on those answers.
# The only way to know them is to ask the implementation everyone else means
# when they say "bc".
#
# It found seven real bugs across its first two runs, every one of which had
# passed a unit test that asserted the same belief the code held:
#
#   1. our bc wrapped at 70 columns; GNU wraps at 69 (bc uses L-2, dc uses L-1)
#   2. our dc's `a` on an empty string answered nothing; GNU answers a NUL byte
#   3. we printed obase>16 with the letters G..Z; GNU writes space-separated
#      zero-padded decimal groups -- `obase=36; 1295` is ` 35 35`
#   4. `x++` as a statement was silent; GNU echoes the pre-increment value
#   5. we interpreted `\n` in every string; only `print` interprets escapes
#   6. our dc's `R` rotated the top n the other way round
#   7. a failed operator ate its operands; GNU leaves them on the stack
#
# `bc_case PROGRAM` and `dc_case PROGRAM` feed the program on stdin to both
# implementations and compare stdout byte for byte, plus the exit status and
# whether there was a diagnostic. `bc_env`/`dc_env` do the same with an
# environment setting, which is how the line-length rules are pinned.
#
# stderr only has to agree about *whether* something was said. Matching GNU's
# wording would be fitting to GNU's message catalogue, not to bc.
#
# xfail_bc/xfail_dc REASON PROGRAM record a difference we keep on purpose. The
# script fails if a plain case differs, and also if an xfail stops differing,
# because that means the recorded reason no longer describes reality.
#
# GNU bc/dc are reached through WSL by default, since there is no native
# Windows build; set GNU_BC/GNU_DC to override (e.g. when running on Linux).
set -u

# Both are built here, from the packages named, rather than picked up from
# `target/`. `bc` in particular used to have a namesake in `coreutils` -- an
# older, separate implementation that also wrote `target/…/debug/bc.exe` -- and
# this harness spent a day reporting 105 differences against it. The two have
# since been merged the right way round (design-decisions.md §359): the crate
# that survives is `coreutils`, the code that survives is the one this harness
# measures. Naming the package is what keeps that honest. See
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS_BC=$(subject_binary coreutils bc "${OURS_BC:-}") || exit 1
OURS_DC=$(subject_binary dc dc "${OURS_DC:-}") || exit 1

if command -v bc >/dev/null 2>&1; then
  GNU_BC=${GNU_BC:-bc}
  GNU_DC=${GNU_DC:-dc}
  WSL=""
else
  # `wsl.exe -e` runs the binary directly, with no shell in between to mangle
  # the program text on its way to stdin.
  WSL=${WSL:-"wsl.exe -d Ubuntu -e"}
  GNU_BC=${GNU_BC:-bc}
  GNU_DC=${GNU_DC:-dc}
fi

# A reference implementation we cannot reach is a reason to say so and stop, not
# a reason to report failures. This has to stay a *skip* rather than an error so
# that the harness can be run from a build that has no WSL without turning into
# a build dependency; the unit tests it seeded stay behind and keep every
# measured value under test either way.
if ! printf '1\n' | $WSL "$GNU_BC" >/dev/null 2>&1 \
  || ! printf '1 p\n' | $WSL "$GNU_DC" >/dev/null 2>&1; then
  echo "calc-diff: GNU bc/dc not reachable (tried: $WSL $GNU_BC / $GNU_DC) -- skipping."
  echo "calc-diff: install them (Debian/Ubuntu: apt-get install bc dc) or set GNU_BC/GNU_DC/WSL."
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0

# compare TOOL PROGRAM [VAR=VALUE...]
compare() {
  local tool="$1" program="$2"; shift 2
  local ours gnu o_out g_out o_err g_err o_rc g_rc
  case "$tool" in
    bc) ours="$OURS_BC"; gnu="$GNU_BC" ;;
    *)  ours="$OURS_DC"; gnu="$GNU_DC" ;;
  esac

  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$(printf '%s\n' "$program" | env "$@" "$ours" 2>"$o_err"); o_rc=$?
  # WSL does not inherit the caller's environment, so a setting has to be
  # carried across with `env` on the far side of the boundary.
  g_out=$(printf '%s\n' "$program" | $WSL env "$@" "$gnu" 2>"$g_err"); g_rc=$?

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

report() {
  local tool="$1" program="$2"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s %s\n' "$tool" "$program"
  else
    fail=$((fail+1))
    printf 'DIFF %s %s\n%s\n' "$tool" "$program" "$REPORT"
  fi
  return 0
}

bc_case() { compare bc "$1"; report bc "$1"; }
dc_case() { compare dc "$1"; report dc "$1"; }
bc_env()  { compare bc "$2" "$1"; report "bc[$1]" "$2"; }
dc_env()  { compare dc "$2" "$1"; report "dc[$1]" "$2"; }

xfail() {
  local tool="$1" reason="$2" program="$3"
  compare "$tool" "$program"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s %s  (%s)\n' "$tool" "$program" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS %s %s\n  now agrees with GNU, so this reason is stale: %s\n' \
      "$tool" "$program" "$reason"
  fi
  return 0
}
xfail_bc() { xfail bc "$1" "$2"; }
xfail_dc() { xfail dc "$1" "$2"; }

# --- bc: arithmetic and the scale rules -------------------------------------
bc_case '2+3'
bc_case '10-4'
bc_case '3*4'
bc_case '7/2'
bc_case '7%3'
bc_case '2^10'
bc_case '2^200'
bc_case '2^1000'
bc_case '(2+3)*4'
bc_case '-5+3'
bc_case '0-0'
bc_case '1/3'
bc_case 'scale=10; 1/3'
bc_case 'scale=20; 1/3'
bc_case 'scale=5; 1/10'
bc_case 'scale=2; 1/2'
bc_case 'scale=0; 1/2'
bc_case 'scale=4; -1/8'
bc_case 'scale=3; 0'
bc_case 'scale=3; 1-1'
bc_case 'scale=10; 8/3'
bc_case '0.1+0.2'
bc_case '1.5*1.5'
bc_case 'scale=2; 1.005*2'
bc_case 'scale=4; 2.5%1.2'
bc_case 'scale=6; sqrt(2)'
bc_case 'scale=20; sqrt(2)'
bc_case 'sqrt(4)'
bc_case 'scale=4; sqrt(0)'
bc_case 'scale=5; 2^-3'
bc_case 'scale=3; 2.5^3'
bc_case '9007199254740993'
bc_case '99999999999999999999*99999999999999999999'
bc_case 'scale=30; 1/7'

# The scale of a product is POSIX's min(a+b, max(scale,a,b)) -- the rule most
# often got wrong, and invisible until an operand has more places than `scale`.
bc_case 'scale=1; 1.23*4.56'
bc_case 'scale=0; 1.23*4.56'
bc_case 'scale=10; 1.23*4.56'
bc_case 'scale=0; 1.5+2.25'
bc_case 'scale=0; 10.5%3'

# --- bc: bases ---------------------------------------------------------------
bc_case 'obase=16; 255'
bc_case 'obase=16; 4096'
bc_case 'obase=2; 10'
bc_case 'obase=8; 64'
bc_case 'ibase=16; FF'
bc_case 'ibase=16; A+A'
bc_case 'ibase=2; 1010'
bc_case 'obase=16; scale=4; 1/2'
bc_case 'obase=16; -255'
bc_case 'obase=36; 1295'

# --- bc: variables, control flow, functions ---------------------------------
bc_case 'x=5; x*2'
bc_case 'x=1; x+=2; x'
bc_case 'i=0; while(i<5){i+=1}; i'
bc_case 'for(i=0;i<3;i++){i}'
bc_case 'define f(x){return(x*2)}; f(21)'
bc_case 'define f(n){if(n<=1)return(1); return(n*f(n-1))}; f(20)'
bc_case 'if(1<2){3}else{4}'
bc_case 'x=10; x--; x'
bc_case 'length(12345)'
bc_case 'scale(1.234)'
bc_case 'a[0]=7; a[0]'
bc_case 'print 1+1, "\n"'
bc_case '"hello\n"'
bc_case 'x=3; -x'

# --- bc: the line-length rule ------------------------------------------------
bc_env 'BC_LINE_LENGTH=10' '2^100'
bc_env 'BC_LINE_LENGTH=20' '2^100'
bc_env 'BC_LINE_LENGTH=3'  '2^40'
bc_env 'BC_LINE_LENGTH=2'  '2^40'
bc_env 'BC_LINE_LENGTH=1'  '2^40'
bc_env 'BC_LINE_LENGTH=0'  '2^100'
bc_env 'BC_LINE_LENGTH=10' 'scale=20; -1/3'
bc_env 'BC_LINE_LENGTH=6'  '12345'
bc_env 'BC_LINE_LENGTH=6'  '123456'

# --- dc: arithmetic ----------------------------------------------------------
dc_case '2 3 + p'
dc_case '10 4 - p'
dc_case '3 4 * p'
dc_case '7 2 / p'
dc_case '7 3 % p'
dc_case '2 10 ^ p'
dc_case '2 200 ^ p'
dc_case '2 1000 ^ p'
dc_case '5 k 1 3 / p'
dc_case '20 k 1 3 / p'
dc_case '5 k 1 10 / p'
dc_case '1 k 1 10 / p'
dc_case '0 k 1 2 / p'
dc_case '3 k 0 p'
dc_case '_5 3 + p'
dc_case '5 _3 * p'
dc_case '10 k 2 v p'
dc_case '4 v p'
dc_case '7 2 ~ f'
dc_case '2 10 3 | p'
dc_case '4 k 1.23 4.56 * p'
dc_case '0 k 1.23 4.56 * p'

# --- dc: the stack -----------------------------------------------------------
dc_case '1 2 3 f'
dc_case '1 2 r f'
dc_case '1 2 3 4 3 R f'
dc_case '5 d * p'
dc_case '1 2 3 z p'
dc_case '1 2 3 c z p'
dc_case '12345 Z p'
dc_case '1.234 X p'
dc_case '[abc] Z p'
dc_case '5 n'
dc_case '65 P'

# --- dc: registers, macros, conditionals -------------------------------------
dc_case '5 sa la p'
dc_case '1 Sa 2 Sa La p La p'
dc_case '[2 3 +] x p'
dc_case '[d 1 - d 1 <F *] sF 5 lF x p'
dc_case '[d 1 - d 1 <F *] sF 30 lF x p'
dc_case '[9 p] sa 5 3 >a'
dc_case '[9 p] sa 3 5 >a'
dc_case '[9 p] sa 5 3 <a'
dc_case '[9 p] sa 3 5 <a'
dc_case '[9 p] sa 3 3 =a'
dc_case '[9 p] sa 3 4 !=a'
dc_case '0 sc [1 + d 5 >L] sL 1 lL x p'
dc_case '[[nested] p] x'
dc_case 'I p O p K p'
dc_case '99 i FF p'
dc_case '16 o 255 p'
dc_case '2 o 10 p'
dc_case '16 o 4096 p'

# --- dc: `a`, which POSIX does not describe at all ---------------------------
dc_case '72 a P 105 a P'
dc_case '[hello] a P'
dc_case '321 a P'
dc_case '65.9 a P'
dc_case '65 a Z p'
dc_case '[] a Z p'
dc_case '0 a Z p'

# --- dc: the line-length rule, which differs from bc's -----------------------
dc_env 'DC_LINE_LENGTH=10' '2 100 ^ p'
dc_env 'DC_LINE_LENGTH=20' '2 100 ^ p'
dc_env 'DC_LINE_LENGTH=2'  '2 40 ^ p'
dc_env 'DC_LINE_LENGTH=1'  '2 40 ^ p'
dc_env 'DC_LINE_LENGTH=0'  '2 100 ^ p'
dc_env 'DC_LINE_LENGTH=10' '20 k _1 3 / p'

# --- bases above sixteen, where a digit no longer fits in one character ------
# Both tools switch to space-separated zero-padded decimal groups. The width is
# whatever the largest digit needs, the space belongs to the digit (so the
# output starts with one), and a `.` stands in for the first fraction digit's
# space. None of that is guessable; all of it is measured here.
bc_case 'obase=36; 1295'
bc_case 'obase=36; 35'
bc_case 'obase=36; 1'
bc_case 'obase=36; 0'
bc_case 'obase=36; -1295'
bc_case 'obase=17; 255'
bc_case 'obase=100; 5'
bc_case 'obase=100; 12345'
bc_case 'obase=1000; 999999'
bc_case 'obase=1000000; 123456789'
bc_case 'scale=4; obase=20; 1/2'
bc_case 'scale=4; obase=20; -1/2'
dc_case '36 o 1295 p'
dc_case '36 o 0 p'
dc_case '36 o _1295 p'
dc_case '17 o 255 p'
dc_case '100 o 12345 p'
dc_case '1000 o 999999 p'
dc_case '4 k 20 o 1 2 / p'

# --- bc: `print` interprets escapes, a bare string literal does not ----------
bc_case 'print "a\nb"'
bc_case 'print "a\tb"'
bc_case 'print "a\qb"'
bc_case 'print "a\rb"'
bc_case 'print "a\ab"'
bc_case 'print "a\fb"'
bc_case 'print "a\vb"'
bc_case 'print "a\zb"'
bc_case 'print "a\\b"'
bc_case '"a\nb"'
bc_case '"lit\\lit"'
bc_case '"hello"'

# --- bc: ++ and -- as statements echo, unlike = ------------------------------
bc_case 'x=10; x--; x'
bc_case 'x=10; x++; x'
bc_case 'x=10; --x; x'
bc_case 'x=10; ++x; x'
bc_case 'x=10; y=x++; y; x'
bc_case 'x=10; (x++); x'
bc_case 'x=10; print x++, "|", x'

# --- dc: R rotates by one, in the direction the sign chooses -----------------
dc_case '1 2 3 3 R f'
dc_case '1 2 3 _2 R f'
dc_case '1 2 3 4 5 9 R f'
dc_case '1 2 3 1 R f'
dc_case '1 2 3 0 R f'

# --- errors ------------------------------------------------------------------
bc_case '1/0'
bc_case 'scale=2; 1/0'
dc_case '1 0 / p'
dc_case '+ p'
dc_case 'p'
dc_case '_1 v p'
# A failed operator leaves its operands where they were, so that the stack
# after the diagnostic still shows what the command was given.
dc_case '1 0 / f'
dc_case '1 0 % f'
dc_case '1 0 ~ f'
dc_case '2 3 0 | f'
dc_case '1 + f'
dc_case '1 | f'
dc_case '[abc] 2 + f'
dc_case '2 [abc] + f'
dc_case '1 [] sa <a f'
dc_case '1 [abc] [] sa <a f'
dc_case '1 2 3 [] sa <a f'
# `v` is the exception: it consumes its operand even when it cannot answer.
dc_case '_4 v f'
dc_case '[abc] v f'
dc_case '1 o 5 p'
dc_case '0 o 5 p'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
