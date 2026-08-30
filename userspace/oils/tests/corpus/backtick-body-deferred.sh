# A backtick body is read at *expansion* time, not with the enclosing source.
# That is the whole difference from `$( … )`, which the parser recurses into
# while reading the word, and it has four visible consequences:
#
#   * a body that never gets expanded is never parsed, so it can be nonsense;
#   * an expanded body is re-parsed on *every* expansion, so a loop reports its
#     error once per iteration (and a `shopt` in between changes the answer);
#   * the failure is not fatal to the reader — the enclosing command still runs,
#     with an empty substitution — it just sets `$?`;
#   * the body's own lines are numbered by a plain offset from the closing
#     backtick's line, not by the rank-based scheme a `$( … )` body gets.
#
# See known-issues.md TD-OILS-CMDSUB-ERR-FATALITY item 1.

echo "=== never expanded, never parsed"
if false; then echo `for`; fi
echo "  status=$?"
case x in y) echo `if ;;` ;; esac
echo "  status=$?"
f() { echo `for do done`; }
echo "  defining it is fine: $?"

echo "=== the reader survives; only the substitution is lost"
echo A`for`B
echo "  status=$? (echo's own)"
x=`if`
echo "  status=$? x=[$x]"

echo "=== a nested \$( … ) body's error is status 1, an ordinary one is 2"
x=`echo $( ! )`
echo "  nested-paren status=$? x=[$x]"
x=`for`
echo "  ordinary status=$? x=[$x]"

echo "=== re-parsed on every expansion, so the error repeats"
for i in 1 2; do echo "  iter $i:"; echo "  [`for`]"; done

echo "=== ... which is also why it cannot be cached: extglob changes the lex"
g() { echo "  [`case x in @(x)) echo matched;; esac`]"; }
g
shopt -s extglob
g
shopt -u extglob

echo "=== line numbers are a plain offset from the closing backtick"
y=`echo $LINENO
echo $LINENO`
echo "  two lines: $y"
q=`echo $LINENO

echo $LINENO`
echo "  blank line between: $q"
z=`
echo $LINENO`
echo "  empty opening remainder: $z"
r=`echo $LINENO; echo $LINENO
echo $LINENO`
echo "  two commands sharing a line: $r"

echo "=== and the diagnostic uses the same numbering"
w=`echo ok

for`
echo "  status=$? w=[$w]"

echo "=== the body is read one line at a time, so what ran before it stands"
w=`echo ok

for`
echo "  status=$? w=[$w]"
w=`echo one
echo two
for
echo three`
echo "  status=$? w=[$w]"
: > side.txt
w=`echo written > side.txt
for`
echo "  status=$? side=[`cat side.txt`]"

echo "=== ... and so a shopt/alias in the body changes how the rest parses"
shopt -s expand_aliases
w=`alias q="echo aliased"
q`
echo "  w=[$w]"
shopt -u expand_aliases

echo "=== an eval or . inside the body reports as itself, not as the body"
w=`eval "for"; echo after`
echo "  status=$? w=[$w]"
printf 'for\n' > inner.sh
w=`. ./inner.sh; echo after`
echo "  status=$? w=[$w]"

echo "=== ... but a fatal \$( … ) body error inside one still ends the body"
printf 'echo \$( ! )\n' > innerp.sh
w=`echo one; . ./innerp.sh; echo three`
echo "  status=$? w=[$w]"

echo "=== the \$(< file) fast path applies here too, and only whole-body"
printf 'hello\nworld\n' > data.txt
echo "  [`< data.txt`]"
echo "  [`0< data.txt`]"
echo "  [`< data.txt < data.txt`]"
echo "  [`< data.txt
echo tail`]"

echo "=== a good body is unaffected"
echo "  [`echo fine`]"
echo "  status=$?"
