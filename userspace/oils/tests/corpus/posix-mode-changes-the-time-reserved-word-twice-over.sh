# posix mode changes `time` in two unrelated ways.
#
# **It takes `time`'s own options away.** bash stops treating `time` as the
# reserved word as soon as the word after it looks like an option, and goes
# looking for an external `time` instead — so `time -p echo hi` is
# `time: command not found` with status 127, and so are `time --`, `time -x`
# and a bare `time -`. The test is on the word *as written*: `time "-p" x`,
# `time \-p x` and `time $D x` keep the reserved word and run a command named
# `-p`, timing it. Since `-p`/`--` are only ever read in that one position,
# taking that position away is what takes the options away.
#
# **And a `time` with no command reports the shell rather than the command.**
# POSIX says a bare `time` writes the shell's own cumulative user and system
# times, so bash drops the `real` line entirely — there is no span to report —
# and prints the other two to two decimals. Outside the mode a bare `time`
# times the null command and prints all three, to three decimals, after a
# leading blank line. bash's test for "no command" is the word list and the
# redirections, so `time x=1` and `time >f` report the ordinary way.
#
# The first of the two is a *parser* change, so the mode has to be entered by a
# command of its own: a `( set -o posix; time -p x )` is parsed whole before any
# of it runs, and reads `-p` as `time`'s option in either shell. The second is
# decided when the report is printed, so it needs no such care.
#
# (Every check below counts lines or matches rather than showing them: the two
# shells cannot be expected to report the same durations. osh reports the CPU
# figures as zero throughout — see known-issues TD-OILS10 — so only the shape is
# compared. `$TIMEFORMAT` is not implemented at all; nothing here sets it.)

echo "=== outside the mode, a bare time times the null command"
{ time ; } 2>t1
echo "  lines:          $(wc -l < t1)"
echo "  leading blank:  $(sed -n '1p' t1 | wc -c)"
echo "  real:           $(grep -c '^real' t1)"
echo "  three decimals: $(grep -c '^real.0m[0-9][0-9]*\.[0-9][0-9][0-9]s$' t1)"

echo "=== and outside it time's own options are read"
{ time -p echo hi ; } 2>e5; rc=$?
echo "  rc=$rc posix-form=$(grep -c '^real [0-9][0-9]*\.[0-9][0-9]$' e5)"

set -o posix

echo "=== in the mode a bare time reports the shell, with no real line"
{ time ; } 2>t2
echo "  lines:          $(wc -l < t2)"
echo "  real:           $(grep -c '^real' t2)"
echo "  user:           $(grep -c '^user' t2)"
echo "  sys:            $(grep -c '^sys' t2)"
echo "  two decimals:   $(grep -c '^user.0m[0-9][0-9]*\.[0-9][0-9]s$' t2)"

echo "=== a redirect or an assignment is a command; a negation is not"
{ time >/dev/null ; } 2>t4; echo "  time >f  real=$(grep -c '^real' t4)"
{ time x=1 ; } 2>t5;        echo "  time x=1 real=$(grep -c '^real' t5)"
{ ! time ; } 2>t6;          echo "  ! time   real=$(grep -c '^real' t6)"

echo "=== the mode takes time's own options away"
{ time -p echo hi ; } 2>e1 >/dev/null; rc=$?
echo "  time -p:  rc=$rc not-found=$(grep -c 'time: command not found' e1)"
{ time -- echo hi ; } 2>e2 >/dev/null; rc=$?
echo "  time --:  rc=$rc not-found=$(grep -c 'time: command not found' e2)"
{ time -x echo hi ; } 2>e3 >/dev/null; rc=$?
echo "  time -x:  rc=$rc not-found=$(grep -c 'time: command not found' e3)"
{ time - ; } 2>e4 >/dev/null; rc=$?
echo "  time -:   rc=$rc not-found=$(grep -c 'time: command not found' e4)"

echo "=== the test is on the word as written"
D=-p
{ time "-p" echo hi ; } 2>e6 >/dev/null; rc=$?
echo "  quoted:   rc=$rc dash-p=$(grep -c -e '-p: command not found' e6) timed=$(grep -c '^real' e6)"
{ time \-p echo hi ; } 2>e7 >/dev/null; rc=$?
echo "  escaped:  rc=$rc dash-p=$(grep -c -e '-p: command not found' e7) timed=$(grep -c '^real' e7)"
{ time $D echo hi ; } 2>e8 >/dev/null; rc=$?
echo "  expanded: rc=$rc dash-p=$(grep -c -e '-p: command not found' e8) timed=$(grep -c '^real' e8)"

echo "=== a word that does not look like an option keeps the reserved word"
{ time echo -p ; } 2>e9; rc=$?
echo "  time echo -p: rc=$rc real=$(grep -c '^real' e9)"
{ time time echo hi ; } 2>e10 >/dev/null; rc=$?
echo "  time time:    rc=$rc reports=$(grep -c '^real' e10)"
{ time "" ; } 2>e11 >/dev/null; rc=$?
echo "  time \"\":      rc=$rc real=$(grep -c '^real' e11)"

set +o posix

echo "=== the mode going away brings them back"
{ time -p echo hi ; } 2>e12 >/dev/null; rc=$?
echo "  rc=$rc posix-form=$(grep -c '^real [0-9]' e12)"
{ time ; } 2>t7; echo "  a bare time: real=$(grep -c '^real' t7)"
