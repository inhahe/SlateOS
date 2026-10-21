#!/usr/bin/env bash
# Differential test: our dc against Ubuntu 24.04's, side by side in WSL.
#
# Ours is `userspace/dc`, GNU dc 1.4.1 (bc 1.07.1) ported.
#
# ## What is compared
#
# For each case: standard output byte for byte, standard error, the exit
# status. Each side runs with an empty environment but `PATH` (its own dc
# first, so `argv[0]` is the bare `dc` on both) and `HOME`, which is a fresh
# directory per run -- some cases put a `.dcrc` in it.
#
# Most programs are run three ways, because dc reads each its own way: as
# `-e`'s expression (a string evaluated whole), as a file operand, and on
# standard input (read as it is evaluated, which is what `?` and `!` see).
#
# ## Deliberate differences
#
# `userspace/dc/src/lib.rs` lists them. The one this harness meets: a file
# name with an unprintable byte is escaped in a diagnostic (xfail).
#
# `DIFF_FAIL_FAST=1` stops at the first difference.
set -u

DIFF_PROG='dc'
DIFF_PKG='dc'
DIFF_NEED='python3 timeout'
DIFF_TIMEOUT=${DIFF_TIMEOUT:-3600}
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

cur=$DIFF_TMP/cur
work=$DIFF_TMP/work
pass=0; fail=0; xfail=0; xpass=0

# One side: a fresh working directory (which is also HOME), the case's
# setup, the command.
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
    if [ -n "${INPUT+x}" ] && [ -n "${INPUT_SET:-}" ]; then
      printf '%s' "$INPUT" > "$DIFF_TMP/input"
      input=$DIFF_TMP/input
    fi
    # shellcheck disable=SC2086
    set -- env -i PATH="$cur:/usr/bin:/bin" HOME="$work" ${ENVS:-} /usr/bin/timeout -k 2 10 "$@"
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
  )
  SETUP=''; INPUT=''; INPUT_SET=''; INFILE=''; ENVS=''; REDIR=''
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$1" "$REPORT"
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then
      echo "dc-diff: stopped at the first difference"
      exit 1
    fi
  fi
  return 0
}

run_case() {
  local label
  label="$(printf '%q ' "$@")${REDIR:-} ${ENVS:-}${SETUP:+ [setup: $SETUP]}${INPUT_SET:+ [stdin: $(printf '%q' "$INPUT")]}"
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

# stdin PROGRAM ARGS...: dc ARGS with PROGRAM on standard input.
stdin() { local p=$1; shift; INPUT=$p; INPUT_SET=1; run_case dc "$@"; }
# file PROGRAM: PROGRAM in a file, named as the operand.
file() { SETUP="printf '%s' $(printf '%q' "$1") > prog"; run_case dc prog; }
# three PROGRAM: the program each of the three ways.
three() { run_case dc -e "$1"; file "$1"; stdin "$1"; }
# raw FORMAT: a program printf makes from FORMAT -- for bytes a shell
# string cannot hold, NUL among them -- as a file operand and on stdin.
raw() {
  SETUP="printf $(printf '%q' "$1") > prog"; run_case dc prog
  SETUP="printf $(printf '%q' "$1") > prog"; INFILE=prog; run_case dc
}

# --- the command line ----------------------------------------------------------
run_case dc --help
run_case dc -h
run_case dc --version
run_case dc -V
run_case dc --he
run_case dc --ver
run_case dc -x
run_case dc --bogus
run_case dc --e
run_case dc -e
run_case dc --expression
run_case dc --file
run_case dc -f
run_case dc -e '1p' -e '2p'
run_case dc --expression='3p' --exp '4p'
run_case dc -e1p
run_case dc -e '5p' -h
run_case dc -V -e '6p'
run_case dc -e 'q' -e '7p'
run_case dc -e '1Q' -e '8p'
SETUP="printf '9p' > a; printf '10p' > b"; run_case dc a b
SETUP="printf '9p' > a; printf '10p' > b"; run_case dc a -e '11p' b
SETUP="printf '9p' > a"; run_case dc -f a -e '12p'
SETUP="printf '9p' > a"; run_case dc --file=a --file a
run_case dc nonexistent
run_case dc -f nonexistent -e '13p'
run_case dc .
SETUP="mkdir d"; run_case dc d -e '14p'
run_case dc /dev/null
INPUT='15p'; INPUT_SET=1; run_case dc -
INPUT='16p'; INPUT_SET=1; run_case dc - -
INPUT='17p'; INPUT_SET=1; run_case dc -e '18p' -
SETUP="mkfifo ff; (printf '19p' > ff &)"; run_case dc ff
SETUP="printf 'q 20p' > a; printf '21p' > b"; run_case dc a b
SETUP="printf '1 0/ 22p' > a"; run_case dc a
SETUP="printf '[unfinished' > a"; run_case dc a -e '23p'
SETUP="printf 'l' > a"; run_case dc a -e '24p'
REDIR='<dir'; run_case dc
REDIR='<&-'; run_case dc -e '25p'
REDIR='<&-'; run_case dc
REDIR='>/dev/full'; run_case dc -e '26p'
REDIR='>/dev/full'; run_case dc --version
REDIR='>/dev/full'; run_case dc --help
REDIR='>/dev/full'; run_case dc -e 'G'
REDIR='>/dev/full'; run_case dc -e '[x]P'
REDIR='>/dev/full'; run_case dc -e '65P'
REDIR='>&-'; run_case dc -e '27p'
REDIR='2>/dev/full'; run_case dc -e '1 0/ 28p'
REDIR='2>/dev/full'; run_case dc -x
REDIR='|head'; run_case dc -e '[[aaaaaaaaaaaaaaaaaaaaaaaa]p lax]sa lax'
xfail_case 'a file name is escaped in a diagnostic' dc "$(printf 'no\nsuch')"

# --- ~/.dcrc ---------------------------------------------------------------------
SETUP="printf '100 sa' > .dcrc"; run_case dc -e 'lap'
SETUP="printf '[from rc]p' > .dcrc"; run_case dc -e '1p'
SETUP="printf 'q' > .dcrc"; run_case dc -e '1p'
SETUP="printf '1 0/' > .dcrc"; run_case dc -e '2p'
SETUP="printf 'l' > .dcrc"; run_case dc -e '3p'
SETUP="mkdir .dcrc"; run_case dc -e '4p'
SETUP="printf '5 6' > .dcrc"; INPUT='f'; INPUT_SET=1; run_case dc

# --- numbers in and out ------------------------------------------------------------
for p in '0p' '_0p' '007p' '1.p' '.5p' '_.5p' '1.50p' '12345678901234567890123456789p' \
         'Ap' 'FFp' '1Fp' 'ABCDEFp' '10.10p' '_ 3p' '_3_4p' '1..2p' '3 _ 4 + p' \
         '2i 1010p' '2i 1.1p' '16i FFp' '16i .8p' '16i A.Ap' '8i 777p' '3i 2.2p' \
         '1i' '17i' '_2i' '[x]i' '2.5i Ip' '16i 10i Ip' '10o 16o Op' '1o' '0o' '_1o' \
         '2o 255p' '8o 255p' '16o 255p 16o _255p' '17o 255p' '100o 12345p' '1000o 1234567p' \
         '2o .5p' '3o 1 3/p' '16o 3k 1 3/p' '20o 2k 1 3/p' '7o 10k 2v p' '16o 15.75p' \
         '100o _12345.678p' '65536o 1234567890123p' '2o 1 100^p' '1.000p' '_0.000p' \
         '5k 1.000000 p' '0.1 0.2 + p' '999999999999999999999 1+p' ; do
  three "$p"
done

# --- line breaks -------------------------------------------------------------------
long='2 300^p 2o 2 200^p 16o 3 300^p [ab]p 10o _2 250^p 1 3 / 100k 1 3 /p'
three "$long"
for v in 0 1 2 3 10 69 70 71 '' ' 20 ' '20 ' 'abc' '0x10' '010' '-5' '2147483648' '99999999999999999999' '20x'; do
  ENVS="DC_LINE_LENGTH=$v"; run_case dc -e "$long"
done

# --- arithmetic ------------------------------------------------------------------
for p in '2 3+p' '2 3-p' '2 3*p' '7 2/p' '7 2%p' '7 2~f' '_7 2/p' '_7 2%p' '7 _2%p' \
         '5k 7 2/p' '5k _7 2%p' '5k 7 3~f' '2k 7.5 2.25*p' '2k 1.111 1.111*p' '0k 1.5 1.5*p' \
         '1 0/' '1 0%' '1 0~' '0 0/' '2 3^p' '2 _3^p' '5k 2 _3^p' '0 _1^p' '2 0^p' '2 3.7^p' \
         '1.5 2^p' '3k 1.5 2^p' '_2 3^p' '2 99999999999^p' '10 2147483647^Z p' \
         '2 10 7|p' '2 10 0|' '2 _1 7|' '2.5 10 7|p' '2 10.5 7|p' '2 10 7.5|p' '_2 3 7|p' '2 3 _7|p' \
         '2vp' '10k 2vp' '_4v' '0vp' '1vp' '0.25vp' '5k 0.0001vp' '100vp' '[x]v' \
         '10k 123456789 vp' '2 3 4' '+' '1+' '[a]1+' '1[a]+' '[a][b]+' '1 2 [c]|' \
         '3 4 5 6 7 8 9 ++++++p' '1 2 3 4 5 6 7 8 9 10 ******* ** ***p' ; do
  three "$p"
done

# --- comparisons -------------------------------------------------------------------
for p in '[yes]sa 1 2<a' '[yes]sa 2 1<a' '[yes]sa 1 1=a' '[yes]sa 2 1>a' '[yes]sa 1 2>a' \
         '[yes]sa 1 2!<a' '[yes]sa 2 1!<a' '[yes]sa 1 1!=a' '[yes]sa 1 2!=a' '[yes]sa 2 1!>a' \
         '1 2<' '1 2!' '[yes]sa 1<a' '[yes]sa [x]1<a' '1 2<b' '[1p]sa 5 5=a 6p' '[q]sa 1 1=a 9p' \
         '! 1 2<a' '[yes]pa 3 3 =a' '!' '!!' '1 2 !<'; do
  three "$p"
done

# --- the stack -----------------------------------------------------------------------
for p in 'p' 'n' 'f' 'c f' 'd' '1 dp f' '1 2 r f' '1 r' 'r' '1 2 3 4 5 3R f' '1 2 3 4 5 _3R f' \
         '1 2 3 9R f' '1 2 3 _9R f' '1 2 3 1R f' '1 2 3 0R f' '1 2 3 [x]R f' 'R' '1 2 3 z f' \
         'zp' '1 2 3 c z p' '[a] [b] [c] f' '1 n 2 n 3 n' '[str]n' '_1.5 n'; do
  three "$p"
done

# --- registers and arrays ------------------------------------------------------------
for p in '5 sa la p' 'la p' 'lap Lap' '1 Sa 2 Sa 3 Sa La p La p La p La p' '1 sa 2 Sa la p La p la p' \
         's' 'l' 'S' 'L' ':' ';' 'La' 'sa' '[x] sa la p' '1 sa 2 sa la p' \
         '10 0:a 20 1:a 0;a p 1;a p 2;a p' '10 _1:a' '_1;a' '10 [x]:a' '[x];a' '1:a' ';a' \
         '5 0:a la p' '5 0:a La' '5 0:a 7 sa la p 0;a p' '5 0:a 7 Sa 0;a p La p 0;a p' \
         '1 0:a 2 Sa 3 0:a 0;a p La 0;a p' '10 2147483648:a' '10 99999999999:a 1' '1 1.5:a 1;a p' \
         '[str] 3:a 3;a p' 'Q ' '9 sx lx 1+ sx lx p'; do
  three "$p"
done
three "$(printf '1 s\001 l\001 p 2 S\001 L\001 p')"
raw '1 s\377 l\377 p'
raw '1 s\000 l\000 p 2 S\000 L\000 p'
three "$(printf '7 s \t l \t p')"

# --- strings and macros ------------------------------------------------------------
for p in '[hello]p' '[]p' '[a[b]c]p' '[a]]p' '[[]p' '[abc' '[a\]p' '[x]Zp' '123Zp' '0Zp' '0.001Zp' \
         '[x]Xp' '1.25Xp' '0Xp' '[x]a p' '65 a p' '321 a p' '_1 a p' '[]a p' '99999999999 a p' \
         '1.9 a p' '[abc]a p' '[2 3+p]x' '5x p' 'x' '[1p]dx x' '[[nested]p]x x' \
         '[1p]sa lax 2p' '[[inner]p]sa [lax [after]p]sb lbx' \
         '0 sc [lc 1+ d sc 5>a]sa lax lcp' '[lc 1+ dsc 3>a]sa 0sc lax lcp' \
         '[1 q 2p]x 3p' '[[1 q 2p]x 3p]x 4p' '[2Q 3p]x 4p' '[[2Q 3p]x 4p]x 5p' \
         '[[[3Q 1p]x 2p]x 3p]x 4p' '[[1Q 1p]x 2p]x 3p' '[0Q]x 1p' '[_1Q]x 1p' '[[x]Q]x 1p' \
         '[9Q]x 2p' '9Q 3p' '[[9Q]x]x 4p' '[lax]sa [1Q]x 5p' 'Q' '[x]Q' '0Q' \
         '[[lbx]sb 2Q]x 6p' '[1p lb x]sb [lbx]sa 3Q' '[2p]sa [lax]x 7p' '[lax]sa [8p]sa lax'; do
  three "$p"
done

# --- P and its bytes -------------------------------------------------------------------
for p in '65 P' '256 P' '16706 P' '_65 P' '65.9 P' '0 P' '[str] P' '[] P' 'P' \
         '2 64^ P' '2 64^ 1- P' '1 P 2 P 10 P' '[a]P[b]P 10P'; do
  three "$p"
done

# --- ? and ! ---------------------------------------------------------------------------
INPUT="$(printf '?\n2 3+p\n4p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf '??\n1p\n2p\n3p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf '?')"; INPUT_SET=1; run_case dc
INPUT="$(printf '[?]x\n5p\n6p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf '9p\n')"; INPUT_SET=1; run_case dc -e '? 1p'
INPUT="$(printf '9p\n')"; INPUT_SET=1; SETUP="printf '?2p' > a"; run_case dc a
INPUT="$(printf '[x]sa\n?\nla p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf '1 2+\n?\np\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf 'G ?\n7p\n')"; INPUT_SET=1; run_case dc
three '!echo hi'
three "$(printf '!echo one\n2p\n!echo three')"
three 'G !echo after'
three '1p !echo after'
three "$(printf '1 2 !< 3p')"
three '!false'
three '!'
raw '!echo a\000b\n5p'
raw '[!echo a\000b\n5p]x'
INPUT="$(printf '!cat\nrest of input\n1p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf '!head -c 3\nabcdef\n2p\n')"; INPUT_SET=1; run_case dc
INPUT="$(printf 'G !echo x\n3p\n')"; INPUT_SET=1; run_case dc

# --- comments, blanks and stray bytes ------------------------------------------------
for p in '# all comment' "$(printf '1p # c\n2p')" "$(printf '1 # c\np')" '1p#c' "$(printf '#\n#\n3p')" \
         'G' 'g' 'Y' '@' '$' '&' '(' ')' ',' '"' "'" 'w' 'H' 'M' 'N' 'T' 'U' 'W' 'b' 'e' 'h' 'j' 'm' 't' 'u' 'y' \
         "$(printf '\001')" "$(printf '\177')" "$(printf '\200\377')" "$(printf '\r1p')" \
         "$(printf '\f\v1p')" "$(printf '1\r2p')" 'GHI JKL'; do
  three "$p"
done
raw '1p\000 2p'
raw '[a\000b]p'
raw '[a\000b]Zp'
raw '1\0002p'
raw '[1\0002p]x 3p'
raw '[a\000b]a p'

# --- a, X, Z, and the values that punt -------------------------------------------------
for p in '2147483647 a p' '2147483648 a p' '4294967296 a p' '99999999999 k' '2147483648 k Kp' \
         '2147483647 k Kp' '99999999999 i' '99999999999 o' '_99999999999 R' '99999999999 Q' \
         '2147483648 Q 1p' '4294967297 R' '[x] 99999999999 :a'; do
  three "$p"
done

# --- unexpected ends ---------------------------------------------------------------------
for p in 'l' 's' 'L' 'S' ':' ';' '<' '=' '>' '1 2 <' '!<' '!=' '!>'; do
  three "$p"
done

# --- seeded random programs (no macros run, no shell, so none loops) -----------------
gen=$DIFF_TMP/gen.py
cat > "$gen" <<'PY'
import random, sys
r = random.Random(20261016)
atoms = ['+', '-', '*', '/', '%', '~', '^', '|', 'v', 'p', 'n', 'f', 'c', 'd', 'r', 'z', 'Z', 'X',
         'a', 'k', 'K', 'i', 'I', 'o', 'O', 'R', 'P', ' ', ' ', ' ', '\n', '_', '.',
         'sa', 'la', 'Sa', 'La', 'sb', 'lb', ':a', ';a', ':b', ';b', 'q', 'Q', '#c\n',
         '[s]', '[a b]', '[', ']', 'G', '\x01', '\xff', 'A', 'F']
for i in range(int(sys.argv[1])):
    parts = []
    for _ in range(r.randint(1, 25)):
        if r.random() < 0.45:
            n = r.choice(['0', '1', '2', '3', '7', '10', '16', '99', '255', '1000', '0.5', '1.25', '_3',
                          str(r.randint(0, 10**r.randint(1, 30))), '.' + str(r.randint(0, 999))])
            parts.append(n + ' ')
        else:
            parts.append(r.choice(atoms))
    sys.stdout.buffer.write(''.join(parts).encode('latin-1') + b'\0')
PY
while IFS= read -r -d '' prog; do
  run_case dc -e "$prog"
  INPUT=$prog; INPUT_SET=1; run_case dc
done < <(python3 "$gen" 300)

echo "dc-diff: $pass passed, $fail differed, $xfail xfail, $xpass xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
