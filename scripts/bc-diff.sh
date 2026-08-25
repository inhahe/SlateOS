#!/usr/bin/env bash
# Differential test: our bc against GNU bc.
#
# ## Why this one was written
#
# Because there was none, and `quit` had been wrong the whole time.
# `known-issues.md` -> TD-B-BC-QUIT-FIRES-AT-THE-WRONG-TIME-AND-HALT-DOES-NOT-EXIST
# records what was found once someone finally ran the two side by side: `quit`
# fired when it was *reached* rather than when it was *read*, and `halt` — the
# one that really does fire when reached — did not exist at all. Both are stated
# outright in the GNU manual. Neither was caught, for the single reason that
# nothing compared the two programs.
#
# ## GNU `bc` has no `-e`, so no case here uses one
#
# Our `bc` accepts `-e EXPR`; GNU bc 1.07.1 does not, and answers
# `invalid option -- 'e'` followed by its usage. That is not a difference in
# behaviour to be adjudicated, it is a different command line, so `-e` is
# exercised by the unit tests in `bc.rs` and never here. Every case below feeds
# its program through a **file operand** or through **standard input**, which
# are the two routes both programs have.
#
# The distinction is not academic: the two routes chunk their input the same way
# but arrive at it differently, and `quit` — whose whole definition is about
# *when the text is read* — is the one construct that can tell them apart. So
# most cases below are run twice, once each way.
#
# ## `-q` on every case, and why the banner is not tested here
#
# Without it GNU prints a version banner, ours prints a different one; the
# comparison would then be of two version strings on every single case rather
# than of the programs. The banner is checked once, as an xfail, and everything
# else runs `-q`.
#
# ## Three kinds of difference, and why they are not one kind
#
# * **xfail** — a difference this project has *decided* to have and expects to
#   still have next year: `--version` names SlateOS. Silent unless it stops
#   reproducing.
# * **KBUG** — a difference nobody wants, which has been found, written up in
#   `known-issues.md`, and not yet fixed. Printed on every run with its tracker
#   key, and does *not* fail the run.
# * **DIFF** — anything else. Fails the run.
#
# The middle category exists because the two obvious alternatives are both
# worse. Leaving a known bug as a plain DIFF makes the harness red on every
# single run, and a harness that is always red is a harness nobody reads —
# which is how `quit` stayed broken. Filing it as an xfail is a lie: it records
# a difference as *intended* when it is in fact debt, and the next reader has no
# way to tell the two apart. So a KBUG is loud, is attributable to a tracker
# entry that describes the fix, and fails the run the moment it starts *passing*
# — because a marker that outlives its bug is a false statement about the tree.
#
# Only the two universal xfails are here — `--help` omits the GNU project's
# `Report bugs to:` block, and `--version` names SlateOS — plus the banner.
# There is no numeric or semantic divergence claimed as intentional.
#
# Run `OURS=/usr/bin/bc ./scripts/bc-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS, every KBUG as KFIXED,
# and nothing else.
set -u

DIFF_PROG=bc
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; kbug=0; kfixed=0

# The tracker key attached to the next comparison, if any. Set by `known_bug`
# and consumed by `report`, so a marker can never be left dangling over the
# case after the one it was written for.
KBUG=

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# --- one comparison -----------------------------------------------------------
# `AGREED` and `REPORT` are set; the caller reports them.
#
# stdout goes through a file rather than a pipe: in `x=$(bc … | od)` the status
# `$?` records is od's, not bc's, and the status is half of what is being
# compared.
compare() {
  local mode="$1" text="$2"; shift 2
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_err=$DIFF_TMP/o.err; g_err=$DIFF_TMP/g.err
  o_bin=$DIFF_TMP/o.out; g_bin=$DIFF_TMP/g.out

  if [ "$mode" = file ]; then
    printf '%b' "$text" > prog.bc
    timeout -k 2 30 env PATH="$bindir/ours" bc -q "$@" prog.bc </dev/null \
      >"$o_bin" 2>"$o_err"; o_rc=$?
    timeout -k 2 30 env PATH="$bindir/gnu"  bc -q "$@" prog.bc </dev/null \
      >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$text" | timeout -k 2 30 env PATH="$bindir/ours" bc -q "$@" \
      >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$text" | timeout -k 2 30 env PATH="$bindir/gnu"  bc -q "$@" \
      >"$g_bin" 2>"$g_err"; g_rc=$?
  fi

  # `od -An -c`, not the text. `bc`'s `print` writes exactly what it is told and
  # nothing more, so trailing newlines -- present or absent -- are part of the
  # answer, and `$(…)` strips them. Comparing the octal dump keeps them.
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  o_err=$(od -An -c <"$o_err"); g_err=$(od -An -c <"$g_err")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" \
            "$(printf '%s' "$o_err" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" \
            "$(printf '%s' "$g_err" | tr -s ' \n' ' ')")
}

# `known_bug KEY` — the next `prog`/`prog_file`/`prog_stdin` is expected to
# differ because of the `known-issues.md` entry named by KEY. The key is
# printed with the row, so the report can be read straight through to the
# write-up without anyone having to remember which bug this was.
known_bug() { KBUG="$1"; }

report() {
  local key=$KBUG; KBUG=
  if [ "$AGREED" = yes ]; then
    if [ -n "$key" ]; then
      # Fails the run on purpose: someone fixed the bug and the marker is now
      # a false claim. Removing it is part of the fix.
      kfixed=$((kfixed+1))
      printf 'KFIXED %s  (%s no longer reproduces -- close it and drop the marker)\n' \
        "$1" "$key"
    else
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
    fi
  elif [ -n "$key" ]; then
    kbug=$((kbug+1))
    printf 'KBUG %s  (%s)\n%s\n' "$1" "$key" "$REPORT"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# `prog LABEL TEXT [ARGS…]` — the same program both ways round, which is the
# only way to see a construct whose meaning depends on how the text arrives.
#
# `KBUG` is re-armed before the second `report`, because `report` consumes it
# and both routes are the same bug.
prog() {
  local label="$1" text="$2"; shift 2
  local key=$KBUG
  compare file  "$text" "$@"; KBUG=$key; report "[file]  $label"
  compare stdin "$text" "$@"; KBUG=$key; report "[stdin] $label"
  KBUG=
}

# One route only, for a case whose other route is not the same question.
prog_file()  { local l="$1" t="$2"; shift 2; compare file  "$t" "$@"; report "[file]  $l"; }
prog_stdin() { local l="$1" t="$2"; shift 2; compare stdin "$t" "$@"; report "[stdin] $l"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_argv() {
  local why="$1"; shift
  local o_all g_all o_rc g_rc o_txt g_txt
  o_all=$DIFF_TMP/xo; g_all=$DIFF_TMP/xg
  timeout -k 2 30 env PATH="$bindir/ours" bc "$@" </dev/null >"$o_all" 2>&1; o_rc=$?
  timeout -k 2 30 env PATH="$bindir/gnu"  bc "$@" </dev/null >"$g_all" 2>&1; g_rc=$?
  o_txt=$(cat "$o_all"); g_txt=$(cat "$g_all")
  if [ "$o_txt" = "$g_txt" ] && [ "$o_rc" = "$g_rc" ]; then
    xpass=$((xpass+1))
    printf 'XPASS bc %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail bc %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# ==============================================================================
# `quit` is a read-time event
# ==============================================================================
# The manual: "quit: when this statement is read, the bc processor is
# terminated, regardless of where the quit statement is found". The granularity
# of "read" is one chunk of input -- one line, or, when a line leaves a brace
# open, every line through the one that closes it. So a `quit` anywhere in a
# chunk cancels the *whole* chunk, including statements written before it, and
# including ones that would never have executed.
#
# Each of these was measured against GNU bc 1.07.1 before being written down.

# The one that already agreed, and the reason the bug survived: with `quit` on
# a line of its own, "read" and "reached" are the same moment.
prog 'quit on its own line'          'print "A"\nquit\nprint "B"\n'

# The one that did not. Everything on the line goes, `print "A"` included.
prog 'quit later on the same line'   'print "A"; quit; print "B"\n'

# `regardless of where the quit statement is found` -- a branch never taken is
# still text that was read.
prog 'quit inside if (0)'            'print "A"\nif (0) { quit }\nprint "B"\n'
prog 'quit inside while (0)'         'print "A"\nwhile (0) { quit }\nprint "B"\n'

# A function body is read as a unit and never executed. GNU stops anyway.
prog 'quit inside a definition'      'define f() { quit }\nprint "before"\n'
prog 'quit in a definition, called'  'define f() { quit }\nprint f()\n'

# A brace-continued construct is one chunk, so the lines above the `quit`
# inside it go with it -- but lines *before* the construct have already run.
prog 'quit inside a multi-line if'   'print "A"\nif (0) {\n quit\n}\nprint "B"\n'
prog 'quit under a loop that ran'    'print "A"\nwhile (0) {\n print "C"\n}\nquit\nprint "B"\n'

# The word only counts as the keyword when it is one.
prog 'the string "quit" is not quit' 'print "quit"\nprint "B"\n'
prog 'an identifier starting quit'   'quitx = 3\nprint quitx\n'

# ==============================================================================
# `halt` is an execution-time event
# ==============================================================================
# The manual again: "halt: … an executed statement". Which is the whole of the
# difference: `halt` in a branch not taken does nothing at all.
prog 'halt on its own line'          'print "A"\nhalt\nprint "B"\n'
prog 'halt later on the same line'   'print "A"; halt; print "B"\n'
prog 'halt inside if (0)'            'print "A"\nif (0) { halt }\nprint "B"\n'
prog 'halt inside if (1)'            'print "A"\nif (1) { halt }\nprint "B"\n'
prog 'halt inside a definition'      'define f() { halt }\nprint "before"\n'
prog 'halt in a definition, called'  'define f() { halt }\nprint "before"\nprint f()\n'
prog 'halt breaking out of a loop'   'i = 0\nwhile (i < 5) { print i; if (i == 2) halt; i += 1 }\n'
prog 'the string "halt" is not halt' 'print "halt"\nprint "B"\n'

# The two together: whichever is *read* first ends it, since `quit` cancels its
# chunk before anything in it executes.
prog 'quit above halt'               'print "A"\nquit\nhalt\nprint "B"\n'
prog 'halt above quit'               'print "A"\nhalt\nquit\nprint "B"\n'
prog 'halt then quit, one line'      'print "A"; halt; quit\n'

# ==============================================================================
# More than one file operand
# ==============================================================================
# `quit` ends the run, not the file: a later operand is not read either.
printf 'print "A"\nquit\n' > q1.bc
printf 'print "A"\nhalt\n' > h1.bc
printf 'print "B"\n'       > two.bc
run_two() {
  local label="$1"; shift
  local o_out g_out o_rc g_rc
  timeout -k 2 30 env PATH="$bindir/ours" bc -q "$@" </dev/null >"$DIFF_TMP/o" 2>&1; o_rc=$?
  timeout -k 2 30 env PATH="$bindir/gnu"  bc -q "$@" </dev/null >"$DIFF_TMP/g" 2>&1; g_rc=$?
  o_out=$(od -An -c <"$DIFF_TMP/o"); g_out=$(od -An -c <"$DIFF_TMP/g")
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ]; then AGREED=yes; else AGREED=no; fi
  REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')")
  report "$label"
}
run_two 'quit in the first of two files' q1.bc two.bc
run_two 'halt in the first of two files' h1.bc two.bc
run_two 'quit in the second of two'      two.bc q1.bc

# ==============================================================================
# Everything else, so this is a harness and not one bug's regression test
# ==============================================================================

# --- arithmetic and scale -----------------------------------------------------
prog 'integer arithmetic'      '1+2\n7-9\n6*7\n\n'
prog 'division truncates'      '7/2\n-7/2\n1/3\n'
prog 'scale changes division'  'scale=10\n1/3\n2/7\n'
prog 'modulo follows scale'    'scale=0\n7%3\n-7%3\nscale=2\n7%3\n'
prog 'exponent'                '2^10\n2^0\n(-2)^3\n'
prog 'scale of a power'        'scale=5\n2^-3\n'
prog 'sqrt'                    'scale=10\nsqrt(2)\nsqrt(0)\n'
prog 'length and scale'        'length(123.456)\nscale(123.456)\nlength(0)\n'
prog 'parentheses and unary'   '-(3+4)\n- -5\n2*-3\n'
prog 'big numbers'             '2^100\n(2^64)-1\n'
known_bug TD-B-BC-RUNTIME-ERROR-WORDING-DIFFERS-FROM-GNU
prog 'division by zero'        '1/0\nprint "after"\n'
known_bug TD-B-BC-RUNTIME-ERROR-WORDING-DIFFERS-FROM-GNU
prog 'sqrt of a negative'      'sqrt(-1)\nprint "after"\n'

# --- ibase / obase ------------------------------------------------------------
prog 'obase hex'               'obase=16\n255\n4096\n'
prog 'obase binary'            'obase=2\n5\n255\n'
prog 'ibase hex'               'ibase=16\nFF\nA0\n'
prog 'ibase then obase'        'ibase=16\nobase=A\nFF\n'

# --- variables, arrays, last --------------------------------------------------
prog 'variables'               'x = 5\ny = x * 2\nprint y, "\\n"\n'
prog 'compound assignment'     'x = 1\nx += 4\nx *= 3\nprint x, "\\n"\n'
prog 'increment operators'     'x = 1\nprint x++, " ", x, "\\n"\nprint ++x, " ", x, "\\n"\n'
prog 'arrays'                  'a[0] = 1\na[1] = 2\nprint a[0] + a[1], "\\n"\n'
prog 'last'                    '2+3\nprint last, "\\n"\n'

# --- control flow -------------------------------------------------------------
prog 'if else'                 'if (1) print "yes\\n" else print "no\\n"\nif (0) print "yes\\n" else print "no\\n"\n'
prog 'while'                   'i=0\nwhile (i<3) { print i; i+=1 }\nprint "\\n"\n'
prog 'for'                     'for (i=0; i<3; i++) print i\nprint "\\n"\n'
prog 'break'                   'for (i=0; i<9; i++) { if (i==3) break; print i }\nprint "\\n"\n'
prog 'continue'                'for (i=0; i<5; i++) { if (i==2) continue; print i }\nprint "\\n"\n'
prog 'nested loops'            'for (i=0;i<3;i++) { for (j=0;j<2;j++) print i,j } \nprint "\\n"\n'

# --- functions ----------------------------------------------------------------
prog 'define and call'         'define f(x) { return (x*2) }\nprint f(21), "\\n"\n'
prog 'recursion'               'define f(n) { if (n<=1) return (1); return (n*f(n-1)) }\nprint f(10), "\\n"\n'
prog 'auto locals'             'define f(x) { auto t; t = x+1; return (t) }\nt = 99\nprint f(1), " ", t, "\\n"\n'
prog 'no explicit return'      'define f() { 1 }\nprint f(), "\\n"\n'
known_bug TD-B-BC-RUNTIME-ERROR-WORDING-DIFFERS-FROM-GNU
prog 'undefined function'      'print f(1)\nprint "after"\n'

# --- print --------------------------------------------------------------------
prog 'print with no newline'   'print "A"\n'
prog 'print several items'     'print "x=", 5, "\\n"\n'
prog 'print escapes'           'print "a\\tb\\nc\\n"\n'
prog 'bare expression prints'  '1+1\n"literal"\n'

# --- -l, the math library -----------------------------------------------------
prog 'mathlib scale default'   'scale\n' -l
prog 'mathlib s and c'         'scale=10\nprint s(0), "\\n", c(0), "\\n"\n' -l
prog 'mathlib e and l'         'scale=10\nprint e(1), "\\n", l(1), "\\n"\n' -l
known_bug TD-B-BC-MATHLIB-ARCTANGENT-IS-INACCURATE
prog 'mathlib a and j'         'scale=10\nprint a(1), "\\n", j(0,1), "\\n"\n' -l
known_bug TD-B-BC-MATHLIB-LOG-ERRORS-WHERE-GNU-SATURATES
prog 'log of zero'             'scale=10\nprint l(0), "\\n"\n' -l

# --- comments and whitespace --------------------------------------------------
prog 'block comment'           '/* a comment */ 1+1\n'
prog 'comment spanning lines'  '/* one\ntwo */ 2+2\n'
prog 'hash comment'            '1+1 # trailing\n2+2\n'
prog 'line continuation'       '1 + \\\n2\n'
prog 'empty input'             ''
prog 'only newlines'           '\n\n\n'
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'no trailing newline'     'print "A"'

# --- errors -------------------------------------------------------------------
# Wording is compared in full. A harness that only asked "did it complain?"
# would pass on every wrong message.
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'syntax error'            'print )\n'
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'unterminated string'     'print "abc\n'
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'unbalanced brace'        'if (1) {\nprint "A"\n'
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'bad character'           '1 $ 2\n'
known_bug TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED
prog 'error then more input'   'print )\nprint "after\\n"\n'

known_bug TD-B-BC-UNAVAILABLE-FILE-NAME-IS-QUOTED-GNU-LEAVES-IT-BARE
prog_file 'a file that is not there' '' nosuch.bc

# ==============================================================================
# Differences on purpose
# ==============================================================================
xfail_argv 'our --help omits the GNU project ancillary block' --help
xfail_argv 'our --version names SlateOS'                      --version
xfail_argv 'our --version names SlateOS'                      -v
# Without `-q` the banner is printed, and the two banners name different
# programs. Every other case above passes `-q` for exactly this reason.
xfail_argv 'our banner names SlateOS'                         -i

printf '\n%d passed, %d differed, %d known bugs, %d differ on purpose' \
  "$pass" "$fail" "$kbug" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
[ "$kfixed" -gt 0 ] && printf ', %d known bugs are FIXED (close them)' "$kfixed"
printf '\n'
# A KBUG does not fail the run; a KFIXED does. See the three-kinds note at the
# top: the whole point of the category is that it lets the harness stay green
# while the debt is still visible, and the one thing that must not stay quiet is
# a marker describing a bug that no longer exists.
[ "$fail" -eq 0 ] && [ "$kfixed" -eq 0 ] || exit 1
