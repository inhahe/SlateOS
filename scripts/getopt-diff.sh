#!/usr/bin/env bash
# Differential test: our `getopt` against util-linux 2.39.3's.
#
# getopt(1) is glibc's getopt_long handed to a shell script, so most of what
# is compared here is getopt_long itself, reached through the one program that
# lets a script choose the option table: permutation, abbreviations and their
# ambiguity, optional and required arguments in every spelling, `+` and `-`
# option strings, a leading `:`, `W;`, getopt_long_only (`-a`), and the
# sentences glibc prints -- all of them, since getopt(1) keeps going after the
# first. Then getopt(1)'s own: its options and their errors, both quoting
# conventions, the old `getopt OPTSTRING ARGS` form, GETOPT_COMPATIBLE,
# POSIXLY_CORRECT, and a write to a full disk.
#
# stdout, stderr and the exit status must all match, except in the cases
# marked as expected differences (`xfail_case`): a name holding a control byte
# is escaped in our diagnostics, where glibc writes the byte raw.
set -u

DIFF_PROG='getopt'
DIFF_PKG='getopt'
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; xfail=0

# $1 = side (ours|gnu), rest = argv; environment additions come from `ENVS`.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 "${ENVS[@]}" PATH="$bindir/$side:$PATH" \
    timeout -k 2 20 getopt "$@"
}

# compare ARGV...: both sides, stdout and stderr to files.
compare() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken; REPORT="  ours rc=$o_rc  gnu rc=$g_rc"; return 0
  fi
  if cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" \
     && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(cat "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(cat "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

ENVS=()
# case_ ARGV...: under the environment in ENVS.
case_() {
  compare "$@"
  report "${ENVS[*]:+${ENVS[*]} }getopt $(printf '%q ' "$@")"
}

# xfail_case WANT-ERR ARGV...: our stderr must be exactly WANT-ERR and
# upstream's something else, stdout and status alike.
xfail_case() {
  local want=$1; shift
  compare "$@"
  local o_err; o_err=$(cat "$DIFF_TMP/o.err")
  if [ "$o_err" = "$want" ] && ! cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" \
     && cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out"; then
    AGREED=yes; xfail=$((xfail + 1))
  else
    AGREED=no
    cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && REPORT="  XPASS: both sides agree now
$REPORT"
  fi
  report "xfail: getopt $(printf '%q ' "$@")"
}

# --- the script's options: every spelling -----------------------------------
case_ -o ab:c:: -- -a -b x -c -cy arg1 arg2
case_ -o ab:c:: -- -abx -acz -ba
case_ -o ab:c:: -l alpha,beta:,gamma:: -- --alpha --beta=1 --beta 2 --gamma --gamma=3 --gamma x
case_ -o ab: -l beta: -- --beta= -b '' ''
case_ -o a -- x -a y -- z -a
case_ -o a -- -- -a
case_ -o a -- - -a -
case_ -o ab -- -ab -ba -aab
case_ -o '' -l verbose,version -- --verb
case_ -o '' -l verbose,version -- --verbo --vers --version
case_ -o '' -l verbose -- --verbose=1 --verb=1
case_ -o '' -l file: -- --file
case_ -o '' -l file: -- --fil
case_ -o '' -l file: -- --file=
case_ -o '' -l file:: -- --file --file=x --file x
case_ -o '' -l 'a,ab,abc' -- --a --ab --abc
case_ -o '' -- --=x
case_ -o '' -l 'x,y' -- --=x
case_ -o '' -l 'x' -- --=x

# --- keeping going after errors ------------------------------------------------
case_ -o a -l foo -- -x -a --bar --foo=1 -q -xax
case_ -o a:b -- -b -a
case_ -o ab -- -xyzb
case_ -o '' -l alpha,alps -- --al --alp=1 --alph
case_ -n myscript -o a -- -x -a --nope
case_ -q -o a -- -x -a --nope
case_ -Q -o a -- -a x
case_ -Q -o a -- -x
case_ -q -Q -o a -- -x

# --- option strings: + - : W; ---------------------------------------------------
case_ -o +a -- x -a
case_ -o -a -- x -a y -- z -a
case_ -o -a: -- x -a y z
case_ -o :a: -- -x -a
case_ -o +:a: -- -x -a
case_ -o -:a -- -x y
case_ -o 'aW;' -l alpha:,alps -- -W alpha=1 -Walpha x -W nope -W al -Walps=2 -W
case_ -o 'W;' -- '-;'
case_ -o 'W' -- -W
case_ -o 'a:' -- '-:'
case_ -o 'aa:' -- -a x

# --- getopt_long_only (-a) ----------------------------------------------------------
case_ -a -o ab -l alpha,beta: -- -alpha -al -ab -beta=1 -x -alpha=2 --al
case_ -a -o ab -l alpha,beta: -- -a -b -beta
case_ -a -o 'a:' -- '-:'
case_ -a -o '' -l verbose,verbose,version -- --verb -verb -verbose
case_ --alternative -o x -l xray -- -x -xr -xray

# --- each -l entry is its own option ------------------------------------------------
case_ -o '' -l foo,foo: -- --fo --foo x
case_ -o '' -l foo -l foo -- --fo
case_ -o '' -l 'alpha beta:	gamma::' -- --alpha --beta 1 --gamma=2
case_ -o '' -l alpha -l beta: -- --alpha --beta x
case_ -o '' -l 'delta:::' -- --delta: --delta:=1
case_ -l ':'
case_ -l 'a,::'
case_ -l ''

# --- quoting -----------------------------------------------------------------------
QX=$'x y!\\\n\'z\tq'
case_ -o a: -- -a "$QX" "$QX"
case_ -s bash -o a: -- -a "$QX"
case_ -s sh -o a: -- -a "$QX"
case_ -s tcsh -o a: -- -a "$QX" "$QX"
case_ --shell=csh -o a: -- -a "$QX"
case_ -s zsh -o a -- -a
case_ -u -o a: -- -a "$QX" "$QX"
case_ --unquoted -o a -- -a 'x y'
case_ -o a: -- -a $'\xff\xfe' $'caf\xc3\xa9' ''

# --- getopt's own options --------------------------------------------------------------
case_ -T
case_ --test -o a
case_ -V
case_ --version
case_ -h
case_ --help
case_ -x
case_ --bogus
case_ --q
case_ --opt a -- -a
case_ --options=a -- -a
case_ --long=alpha --opt= -- --alpha
case_ -o
case_ -n
case_ -o a
case_ -o a --
case_ -n name
case_ --
case_ -o a -o b -- -a -b
case_ -n one -n two -o '' -- -x
case_ -s tcsh -s bash -o a: -- -a 'x y'
case_ -o a x -a
case_ -- ab -a x
case_ -a -- ab -ab x

# --- the old form, and the environment ---------------------------------------------
case_
case_ ab:c x -a -b y z
case_ ab -a 'x y' -c
case_ +a x -a
case_ -+-a -a
ENVS=(GETOPT_COMPATIBLE=1)
case_
case_ -ab x -a
case_ -o a -- -a
case_ --+ab -b
ENVS=(POSIXLY_CORRECT=1)
case_ -o a -- x -a
case_ -o +a -- x -a
case_ -o -a -- x -a
case_ ab x -a
ENVS=()

# --- the name errors are reported under ---------------------------------------------
# argv[0] as given, in getopt's own errors; its last component in the rest.
mkdir -p "$DIFF_TMP/path"
path_case() {
  local o_rc g_rc
  ln -sfn "$(readlink -f "$bindir/ours/getopt")" "$DIFF_TMP/path/getopt"
  env LC_ALL=C.UTF-8 "$DIFF_TMP/path/getopt" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  ln -sfn "$(readlink -f "$bindir/gnu/getopt")" "$DIFF_TMP/path/getopt"
  env LC_ALL=C.UTF-8 "$DIFF_TMP/path/getopt" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  if cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" \
     && [ "$o_rc" = "$g_rc" ]; then AGREED=yes; else AGREED=no; fi
  REPORT=$(printf '  ours (rc=%s): %q\n  gnu  (rc=%s): %q' \
    "$o_rc" "$(cat "$DIFF_TMP/o.out" "$DIFF_TMP/o.err")" "$g_rc" "$(cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err")")
  report "by path: getopt $(printf '%q ' "$@")"
}
path_case -x
path_case -o a -- -x
path_case --help
path_case -V
path_case

# --- a full disk ----------------------------------------------------------------------
if [ -w /dev/full ]; then
  full_case() {
    local o_rc g_rc
    run_side ours "$@" >/dev/full 2>"$DIFF_TMP/o.err"; o_rc=$?
    run_side gnu "$@" >/dev/full 2>"$DIFF_TMP/g.err"; g_rc=$?
    if cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then AGREED=yes; else AGREED=no; fi
    REPORT=$(printf '  ours (rc=%s): %q\n  gnu  (rc=%s): %q' \
      "$o_rc" "$(cat "$DIFF_TMP/o.err")" "$g_rc" "$(cat "$DIFF_TMP/g.err")")
    report "to /dev/full: getopt $(printf '%q ' "$@")"
  }
  full_case -o a -- -a x
  full_case -V
fi

# --- expected differences: control bytes in a name -------------------------------------
xfail_case 'myscript: unrecognized option '"'"'--a\nb'"'" -o '' -n myscript -- $'--a\nb'
xfail_case 'my\033[31mname: invalid option -- '"'"'x'"'" -n $'my\e[31mname' -o '' -- -x

echo "getopt-diff: $pass passed ($xfail of them expected differences), $fail failed, $broken broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
