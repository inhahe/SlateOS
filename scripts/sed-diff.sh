#!/usr/bin/env bash
# Differential test: our sed against the host's GNU sed.
#
# Each case is one line of the form `SCRIPT<TAB>INPUT-NAME<TAB>ARGS...`.
# Both seds get identical argv and identical stdin; output and status are
# compared byte for byte.  This is the check that a unit test cannot make,
# because a unit test asserts what we believe rather than what sed does.
#
# stdout and the exit status are compared byte for byte; stderr only has to
# agree about *whether* there was a diagnostic. Matching GNU's `char 7:`
# offsets exactly would be fitting to its parser's internals, not to sed.
set -u

# Our sed is a native Windows binary, so MSYS would helpfully rewrite an
# argument that looks like a path — turning the script `/b/p` into `B:\p`.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/sed.exe"}
GNU=${GNU:-sed}

declare -A INPUTS=(
  [abc]=$'a\nb\nc\n'
  [nums]=$'1\n2\n3\n4\n5\n'
  [words]=$'foo bar\nbaz foo\nqux\n'
  [blanks]=$'a\n\n\n\nb\n\nc\n'
  [nonl]=$'a\nb'
  [empty]=''
  [dup]=$'x\nx\ny\ny\ny\nz\n'
  [paths]=$'/usr/bin\n/tmp/x\nrelative\n'
  [mixed]=$'Alpha1\nbeta22\nGAMMA333\n'
)

pass=0; fail=0

run_case() {
  local input_name="$1"; shift
  local data="${INPUTS[$input_name]}"

  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$(printf '%s' "$data" | "$OURS" "$@" 2>"$o_err"); o_rc=$?
  g_out=$(printf '%s' "$data" | "$GNU" "$@" 2>"$g_err"); g_rc=$?

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_loud" = "$g_loud" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   [%s] %s\n' "$input_name" "$*"
  else
    fail=$((fail+1))
    printf 'DIFF [%s] %s\n' "$input_name" "$*"
    printf '  ours (rc=%s): %s  {%s}\n' "$o_rc" \
      "$(printf '%s' "$o_out" | tr '\n' '|')" "$(tr '\n' '|' <"$o_err")"
    printf '  gnu  (rc=%s): %s  {%s}\n' "$g_rc" \
      "$(printf '%s' "$g_out" | tr '\n' '|')" "$(tr '\n' '|' <"$g_err")"
  fi
  rm -f "$o_err" "$g_err"
}

# --- substitution -----------------------------------------------------------
run_case words 's/foo/FOO/'
run_case words 's/foo/FOO/g'
run_case words 's/o/0/2'
run_case words 's/o/0/2g'
run_case words 's/^/> /'
run_case words 's/$/ </'
run_case words 's/[abc]/./g'
run_case words 's/[^abc]/./g'
run_case words 's/\(b\)\(a\)/\2\1/'
run_case words 's/.*/[&]/'
run_case words 's/a/[\&]/'
run_case words 's/a*/-/g'
run_case words 's/x*/-/g'
run_case words 's/.*/\U&/'
run_case mixed 's/.*/\L&/'
run_case mixed 's/\(.\)\(.*\)/\u\1\2/'
run_case mixed 's/\(.*\)/\U\1\E!/'
run_case paths 's|/usr|/opt|'
run_case paths 's,/tmp,/var,'
run_case paths 's/[/]/:/g'
run_case paths 's/[^/]*$/LAST/'
run_case words 's/[[:upper:]]/U/g'
run_case mixed 's/[[:digit:]][[:digit:]]*//'
run_case words 's/FOO/x/I'
run_case words 's/o/0/p'
run_case words -n 's/o/0/p'
run_case words 's/a\+/X/'
run_case words -E 's/a+/X/'
run_case words -E 's/(foo|baz)/[\1]/'
run_case words 's/foo\|baz/X/'

# --- addresses --------------------------------------------------------------
run_case nums '2d'
run_case nums '2,4d'
run_case nums '$d'
run_case nums '1,$d'
run_case nums -n '2,+2p'
run_case nums -n '2,~4p'
run_case nums -n '4,~4p'
run_case nums -n '1~2p'
run_case nums -n '0~2p'
run_case abc -n '/b/p'
run_case abc -n '/b/,/c/p'
run_case abc -n '/a/,/zz/p'
run_case abc -n '0,/a/p'
run_case abc -n '1,/a/p'
run_case abc '2!d'
run_case abc -n '/B/Ip'
run_case paths -n '\%/usr%p'
run_case abc -n '$='
run_case abc '='

# --- the rest of the command set -------------------------------------------
run_case abc 'p'
run_case abc -n 'p'
run_case abc 'd'
run_case abc '1!G;h;$!d'
run_case abc '$!N;s/\n/ /'
run_case abc 'N;s/\n/ /'
run_case abc -n 'n;p'
run_case blanks '/^$/{N;/^\n$/D}'
run_case dup '$!N;/^\(.*\)\n\1$/!P;D'
run_case abc ':a;s/a/b/;ta'
run_case words ':a;s/  / /;ta'
run_case abc 's/a/x/;T end;s/$/ (changed)/;:end'
run_case abc '/b/{s/b/B/;p}'
run_case abc 'y/abc/xyz/'
run_case abc '2i\
before'
run_case abc '1a after'
run_case abc '2c\
new'
run_case abc '1,2c\
new'
run_case abc '2q'
run_case abc '2Q'
run_case abc '2q5'
run_case abc 'h;s/./X/;G'
run_case abc 'H;$!d;x;s/\n/,/g'
run_case abc -n 'x;p'
run_case abc -n '$!{h;d};x;p'
run_case abc '#n
p'
run_case abc '# comment
p'

# --- odd inputs -------------------------------------------------------------
run_case nonl 'p'
run_case nonl 's/b/B/'
run_case nonl '$!N;s/\n/-/'
run_case empty 'p'
run_case empty 's/a/b/'
run_case abc -s 'p'
run_case abc -n '$p'

# --- errors -----------------------------------------------------------------
run_case abc 'Z'
run_case abc 's/a'
run_case abc '{p'
run_case abc 'b nowhere'
run_case abc 'y/ab/x/'
run_case abc 's/[a/x/'

printf '\n%d passed, %d differed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
